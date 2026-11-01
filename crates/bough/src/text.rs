//! Text, as bough-text iteration 2 recommends (its RESULTS "Recommended text pipeline"), with the glyph pool
//! living in the fused region so the GPU expands the runs itself:
//!
//! * **one line** (the common UI label): a retained `Label`. A new value is a patch: digits that stay digits
//!   rewrite their `u16` entry ids and OR a cached tile span into the damage plane (the numeric operand
//!   renders the value straight into the label's 16-byte vector, no `String`); other edits re-step from the
//!   first changed byte with O(1) line marks; ligatures and Latin-1 go through the token table.
//! * **several lines** (`\n`): a ring of lines (`bough_text::lines::LineSet`, round 3). Every line is shaped
//!   alone into its own pool block (byte slots: ligatures and Latin-1 included) and drawn by one run header; a
//!   new value is matched line by line first (common prefix, a shift of lines scrolled off the top, common
//!   suffix), so a log tick shapes one line and slides the headers, and an edit shapes the changed line only.
//! * **wrapped by layout**: the text is shaped once into a block (byte slots); each line is one 32-byte run
//!   header, a *window* onto the block. The record draws the whole range of headers.
//! * **beyond Latin-1** (and per-glyph colour, `glyph_alpha`): 48-byte glyph instances in an instance run,
//!   shaped by the table kernel per ASCII segment and by rustybuzz per non-Latin-1 chunk (cached). This is the
//!   only rustybuzz use left; everything the token table covers goes through the pool.
//!
//! Damage is produced at update time as tile marks (labels, line windows) into bough-core's current plane.
use crate::rt::*;
use bough_core::{Handle, NRect};
use bough_metal::fused::{pay_irun, pay_runs};
use bough_metal::instance::Instance;
use bough_text::atlas::{GlyphCache, Instance as TInstance, LineBounds, StrikeId, TokStrike};
use bough_text::pool::{pen_px, Label};
use bough_text::lines::{LineSet, LineStyle};
use bough_text::window::Shaped;
use bough_text::{GlyphRun, Shaper, SimpleScript};

pub struct TextSys {
    pub shaper: Shaper,
    pub cache: GlyphCache,
    pub scratch: Vec<TInstance>,
    face: rustybuzz::Face<'static>,
    /// Font-unit vertical metrics (hhea).
    pub ascent: f32,
    pub descent: f32,
    pub upem: f32,
    strikes: Vec<(u32, StrikeId)>,
    /// Lines that needed rustybuzz for a non-ASCII chunk (segmented) or for the whole line.
    pub segmented: u64,
    pub fallbacks: u64,
    run: GlyphRun,
    seg: GlyphRun,
    /// Shaped non-Latin-1 chunks (gids, x offsets, width in font units), by text.
    chunks: Vec<(String, Vec<u16>, Vec<i32>, i32)>,
    /// Scratch for multi-line shaping (newlines as spaces).
    pub bytes: Vec<u8>,
}

impl TextSys {
    /// `toks`: the (carved, GPU-visible) column of strike token tables.
    pub fn new(font: &[u8], toks: bough_mem::Col<TokStrike>) -> TextSys {
        let script = SimpleScript::build(font, 0).expect("font has no simple-script table");
        let leaked: &'static [u8] = Box::leak(font.to_vec().into_boxed_slice());
        let tf = ttf_parser::Face::parse(leaked, 0).expect("font");
        let (ascent, descent, upem) = (tf.ascender() as f32, -(tf.descender() as f32), tf.units_per_em() as f32);
        TextSys {
            shaper: Shaper::new(script),
            cache: GlyphCache::with_tok_column(font, toks),
            scratch: Vec::with_capacity(1024),
            face: rustybuzz::Face::from_slice(leaked, 0).expect("rustybuzz face"),
            ascent,
            descent,
            upem,
            strikes: Vec::new(),
            segmented: 0,
            fallbacks: 0,
            run: GlyphRun::default(),
            seg: GlyphRun::default(),
            chunks: Vec::new(),
            bytes: Vec::new(),
        }
    }

    /// Strike id for a pixel size (cached by the size's bits).
    pub fn strike(&mut self, size_px: f32) -> StrikeId {
        let key = size_px.to_bits();
        if let Some(&(_, s)) = self.strikes.iter().find(|(k, _)| *k == key) {
            return s;
        }
        let s = self.cache.strike(size_px);
        // ASCII-complete from the start (every path may draw any printable byte without a readiness check)
        let tt = self.shaper.script.tokens.as_ref().expect("token model");
        self.cache.prepare_tokens(s, &tt.gid, 0x20..0x7F);
        self.strikes.push((key, s));
        s
    }

    /// Make every token of `ents` drawable in `strike` (a Latin-1 glyph or a ligature seen for the first time:
    /// rasterized once). Cold: called only when the shaper reports tokens beyond ASCII.
    #[cold]
    pub fn ready(&mut self, strike: StrikeId, ents: &[u16]) {
        let tt = self.shaper.script.tokens.as_ref().expect("token model");
        let ts = self.cache.tok(strike);
        if ents.iter().all(|&t| ts.ready[t as usize] != 0) {
            return;
        }
        self.cache.prepare_tokens(strike, &tt.gid, ents.iter().copied());
    }

    /// Baseline offset below the top of a line box of height `lh` (CSS half-leading), px.
    pub fn baseline(&self, size_px: f32, lh_px: f32) -> f32 {
        let content = (self.ascent + self.descent) / self.upem * size_px;
        (lh_px - content) * 0.5 + self.ascent / self.upem * size_px
    }

    /// The strike's conservative line bounds (O(1) line damage and record ink).
    pub fn bounds(&mut self, strike: StrikeId) -> LineBounds {
        let tt = self.shaper.script.tokens.as_ref().expect("token model");
        self.cache.line_bounds(strike, tt, self.shaper.script.upem)
    }

    /// Non-Latin-1 chunk `s` shaped by rustybuzz once; index into `chunks`.
    fn chunk(&mut self, s: &str) -> usize {
        if let Some(i) = self.chunks.iter().position(|c| c.0 == s) {
            return i;
        }
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(s);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&self.face, &[], buf);
        let (infos, pos) = (out.glyph_infos(), out.glyph_positions());
        let (mut gids, mut xs, mut x) = (Vec::new(), Vec::new(), 0i32);
        for (i, p) in infos.iter().zip(pos) {
            gids.push(i.glyph_id as u16);
            xs.push(x + p.x_offset);
            x += p.x_advance;
        }
        self.chunks.push((s.to_owned(), gids, xs, x));
        self.chunks.len() - 1
    }

    /// Shape `line` into `self.run` by segments: maximal ASCII stretches by the table kernel, maximal
    /// non-ASCII stretches by a cached rustybuzz run of just that stretch. False if an ASCII segment holds
    /// a trigger: the caller shapes the whole line with rustybuzz. Kerning across a segment boundary is not
    /// applied (the pair table has no entry for a non-ASCII neighbour).
    fn segment(&mut self, line: &str, size_px: f32) -> bool {
        let bytes = line.as_bytes();
        let mut seg = std::mem::take(&mut self.seg);
        let mut run = std::mem::take(&mut self.run);
        run.gid.clear();
        run.x.clear();
        run.cluster.clear();
        run.identity_clusters = true;
        run.upem = self.upem as u16;
        run.size = size_px;
        let (mut i, mut x) = (0usize, 0i32);
        let mut ok = true;
        while i < bytes.len() {
            let ascii = bytes[i] < 128;
            let mut j = i;
            while j < bytes.len() && (bytes[j] < 128) == ascii {
                j += 1;
            }
            if ascii {
                if !self.shaper.shape_into(&line[i..j], size_px, &mut seg) {
                    ok = false;
                    break;
                }
                run.gid.extend_from_slice(&seg.gid);
                run.x.extend(seg.x.iter().map(|v| v + x));
                x += seg.width;
            } else {
                let c = self.chunk(&line[i..j]);
                let (_, g, xs, w) = &self.chunks[c];
                run.gid.extend_from_slice(g);
                run.x.extend(xs.iter().map(|v| v + x));
                x += *w;
            }
            i = j;
        }
        run.width = x;
        self.seg = seg;
        self.run = run;
        ok
    }

    /// Append the 48-byte glyph instances of one line (pen origin `ox`, baseline `oy`, px) to `scratch`.
    pub fn line(&mut self, strike: StrikeId, size_px: f32, line: &str, ox: f32, oy: f32, color: u32) -> usize {
        if let Some(n) = self.cache.shape_instances_ascii(&self.shaper, strike, line, ox, oy, color, &mut self.scratch) {
            return n;
        }
        if self.segment(line, size_px) {
            self.segmented += 1;
        } else {
            self.fallbacks += 1;
            self.shape_rb(line, size_px);
        }
        let run = std::mem::take(&mut self.run);
        let n = self.cache.instances(strike, &run, ox, oy, color, &mut self.scratch);
        self.run = run;
        n
    }

    fn shape_rb(&mut self, text: &str, size_px: f32) {
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&self.face, &[], buf);
        let r = &mut self.run;
        r.gid.clear();
        r.x.clear();
        r.cluster.clear();
        r.identity_clusters = true;
        r.upem = self.upem as u16;
        r.size = size_px;
        let mut x = 0i32;
        for (i, p) in out.glyph_infos().iter().zip(out.glyph_positions()) {
            r.gid.push(i.glyph_id as u16);
            r.x.push(x + p.x_offset);
            x += p.x_advance;
        }
        r.width = x;
    }

    /// Shape `text` into `out` (font units): the table shaper when it covers the string, else rustybuzz
    /// (x advances and glyph ids). Layout's measure path for text the pool block cannot hold.
    pub fn shape_run(&mut self, text: &str, size_px: f32, out: &mut GlyphRun) {
        if self.shaper.shape_into(text, size_px, out) {
            return;
        }
        self.fallbacks += 1;
        self.shape_rb(text, size_px);
        std::mem::swap(&mut self.run, out);
    }
}

// =============================================================================== text nodes

/// How a text node is represented (see the module docs).
#[derive(Default)]
pub enum TShape {
    #[default]
    None,
    Label(Box<Label>),
    /// Explicit lines (`\n`): a ring of lines, one block and one run header per line.
    Set(Box<LineSet>),
    /// Run headers `[first, first + cap)`, `n` in use, windows onto `block` (layout's wrapped leaves).
    Lines { block: Option<Shaped>, owned: bool, first: u32, cap: u32, n: u32, widths: Vec<i32>, spans: Vec<(u32, u32)> },
    /// An instance run of glyph instances (`Rt::runs` index).
    Glyphs { run: u32 },
}

#[derive(Default)]
pub struct TextNode {
    pub text: String,
    pub strike: u32,
    pub size_px: f32,
    pub line_h: f32,
    pub color: u32,
    /// Pen origin of the first line, space-local px (`oy` = its baseline, integral).
    pub ox: f32,
    pub oy: f32,
    pub space: u16,
    pub handle: Handle,
    /// The record's current (absolute) ink.
    pub ink: NRect,
    pub shape: TShape,
    /// Per-glyph colour (instance representation even for covered text).
    pub per_glyph: bool,
    /// The format of a numeric operand (`sig!(..).num(f, fmt)`).
    pub num: Option<bough_text::numeric::NumFmt>,
}

/// Release a text node's pool resources (its record is removed by the caller's scope).
pub(crate) fn release(rt: &mut Rt, t: TextNode) {
    match t.shape {
        TShape::None => {}
        TShape::Label(lb) => {
            let Rt { glyphs, .. } = rt;
            let mut sink = NoMarks;
            glyphs.free_label(*lb, &mut sink);
        }
        TShape::Lines { block, owned, first, cap, .. } => {
            rt.glyphs.free_run_range(first, cap);
            if let (Some(b), true) = (block, owned) {
                rt.glyphs.free_shaped(b);
            }
        }
        TShape::Set(mut set) => set.free(&mut rt.glyphs),
        TShape::Glyphs { run } => rt.free_run(run),
    }
}

/// A mark sink that drops marks (a released or freshly mounted record is damaged through its ink).
struct NoMarks;
impl bough_text::pool::Marks for NoMarks {
    fn rows(&self) -> i32 {
        0
    }
    fn or_rows(&mut self, _: i32, _: i32, _: u128) {}
}

/// Ink of a line window `[0, w]` (font units) at baseline `oy`, pen origin `ox`, in space `sp`.
fn line_ink(b: &LineBounds, scale: f32, ox: f32, oy: i32, w: i32, sp: [i32; 2]) -> NRect {
    if w <= 0 {
        return NRect::EMPTY;
    }
    let oxh = ox + 0.5;
    NRect::new(pen_px(0, scale, oxh) + sp[0] + b.left, oy + sp[1] + b.top, pen_px(w, scale, oxh) + sp[0] + b.over, oy + sp[1] + b.bottom)
}

impl Rt {
    fn tscale(&self, t: &TextNode) -> f32 {
        t.size_px / self.text.shaper.script.upem as f32
    }

    /// Build text node `ti`'s representation for `t.text` (mount, or a change of representation).
    /// Returns the record payload; sets `t.ink`.
    pub(crate) fn text_build(&mut self, ti: u32) -> u32 {
        let (single, covered) = {
            let t = &self.texts[ti as usize];
            (!t.text.contains('\n'), !t.per_glyph)
        };
        if single && covered {
            if let Some(p) = self.build_label(ti) {
                return p;
            }
        } else if covered {
            if let Some(p) = self.build_set(ti) {
                return p;
            }
        }
        self.build_glyphs(ti)
    }

    /// The draw style of text node `ti`'s line set.
    fn set_style(&self, ti: u32) -> LineStyle {
        let t = &self.texts[ti as usize];
        LineStyle { ox: t.ox, oy: t.oy, lh: t.line_h, scale: self.tscale(t), color: t.color, space: t.space, strike: t.strike as u16 }
    }

    /// Ink of a line set: the union of its lines' extents.
    fn set_ink(&mut self, ti: u32) -> NRect {
        let strike = self.texts[ti as usize].strike as StrikeId;
        let b = self.text.bounds(strike);
        let TShape::Set(set) = &self.texts[ti as usize].shape else { unreachable!() };
        let st = set.style;
        let sp = self.glyphs.spaces[st.space as usize];
        let mut ink = NRect::EMPTY;
        for (i, l) in set.lines.iter().enumerate() {
            ink = ink.union(line_ink(&b, st.scale, st.ox, (st.oy + i as f32 * st.lh).round() as i32, l.width, sp));
        }
        ink
    }

    /// Ready the tokens beyond ASCII a set's lines use (cold).
    #[cold]
    fn ready_set(&mut self, ti: u32) {
        let strike = self.texts[ti as usize].strike as StrikeId;
        let TShape::Set(set) = &self.texts[ti as usize].shape else { return };
        let mut ents = Vec::new();
        for l in &set.lines {
            if l.base != u32::MAX {
                ents.extend_from_slice(&self.glyphs.ent[l.base as usize..(l.base + l.len) as usize]);
            }
        }
        self.text.ready(strike, &ents);
    }

    /// Several lines (`\n`): a ring of lines.
    fn build_set(&mut self, ti: u32) -> Option<u32> {
        let style = self.set_style(ti);
        let strike = self.texts[ti as usize].strike as StrikeId;
        let b = self.text.bounds(strike);
        let mut set = Box::new(LineSet::new(style));
        let text = std::mem::take(&mut self.texts[ti as usize].text);
        let r = set.set_text::<[u128]>(&mut self.glyphs, &self.text.shaper, &text, &b, None);
        self.texts[ti as usize].text = text;
        let st = r.ok()?;
        let (first, cap) = (set.first, set.cap);
        self.texts[ti as usize].shape = TShape::Set(set);
        if st.exotic {
            self.ready_set(ti);
        }
        let ink = self.set_ink(ti);
        self.texts[ti as usize].ink = ink;
        Some(pay_runs(first, cap))
    }

    fn build_label(&mut self, ti: u32) -> Option<u32> {
        let strike = self.texts[ti as usize].strike as StrikeId;
        let (size_px, ox, oy, color, space) = {
            let t = &self.texts[ti as usize];
            (t.size_px, t.ox, t.oy, t.color, t.space)
        };
        let text = std::mem::take(&mut self.texts[ti as usize].text);
        let Rt { glyphs, text: ts, .. } = self;
        let mut lb = glyphs.alloc_label(&ts.shaper, strike, size_px, [ox, oy], color, space, &mut ts.cache);
        let ok = glyphs.set_text_d::<false, _>(&ts.shaper, &mut ts.cache, &mut lb, &text, &mut NoMarks);
        self.texts[ti as usize].text = text;
        if !ok {
            glyphs.free_label(lb, &mut NoMarks);
            return None;
        }
        let run = lb.run;
        let ink = line_ink(&lb.bounds, size_px / self.text.shaper.script.upem as f32, ox, oy as i32, lb.width(), self.glyphs.spaces[space as usize]);
        let t = &mut self.texts[ti as usize];
        t.ink = ink;
        t.shape = TShape::Label(Box::new(lb));
        Some(pay_runs(run, 1))
    }

    /// Point text node `ti`'s line windows at `spans` of `block` (lines `i` at baseline `oy + i * line_h`);
    /// with `damage`, mark each line whose window changed (old and new extents, O(1) each). Grows the
    /// header range if needed (the payload changes then). Updates `t.ink`.
    pub(crate) fn lines_point(&mut self, ti: u32, block: Shaped, spans: &[(u32, u32)], damage: bool) {
        let strike = self.texts[ti as usize].strike as StrikeId;
        let b = self.text.bounds(strike);
        let scale = self.tscale(&self.texts[ti as usize]);
        let (ox, oy, lh, color, space) = {
            let t = &self.texts[ti as usize];
            (t.ox, t.oy, t.line_h, t.color, t.space)
        };
        let n = spans.len();
        let TShape::Lines { first, cap, widths, spans: old_spans, n: old_n, block: old_block, .. } = &mut self.texts[ti as usize].shape else { unreachable!() };
        let (mut first, mut cap, old_n) = (*first, *cap, *old_n as usize);
        let mut widths = std::mem::take(widths);
        let old_spans_v = std::mem::take(old_spans);
        let old_base = old_block.map(|b| b.base);
        if damage {
            // old extents of every line that changes (headers still describe the old windows)
            for i in 0..old_n {
                let same = i < n && old_base == Some(block.base) && old_spans_v[i] == spans[i];
                if !same {
                    let Rt { glyphs, bough, .. } = self;
                    glyphs.mark_window(first + i as u32, widths[i], &b, &mut CoreMarks(bough.plane_mut()));
                }
            }
        }
        if n as u32 > cap {
            self.glyphs.free_run_range(first, cap);
            let (f, c) = self.glyphs.alloc_run_range(n);
            (first, cap) = (f, c);
            let h = self.texts[ti as usize].handle;
            self.bough.set_payload_handle(h, pay_runs(first, cap));
        }
        widths.clear();
        let sp = self.glyphs.spaces[space as usize];
        let mut ink = NRect::EMPTY;
        let x = &self.glyphs.x;
        for (i, &(a, e)) in spans.iter().enumerate() {
            let w = if e > a {
                let end = if (e as usize) < block.len as usize { x[(block.base + e) as usize] } else { block.width };
                end.wrapping_sub(x[(block.base + a) as usize])
            } else {
                0
            };
            widths.push(w);
            let base = (oy + i as f32 * lh).round();
            ink = ink.union(line_ink(&b, scale, ox, base as i32, w, sp));
        }
        let strike16 = strike as u16;
        for (i, &(a, e)) in spans.iter().enumerate() {
            let r = first + i as u32;
            let base = (oy + i as f32 * lh).round();
            let h = &mut self.glyphs.runs[r as usize];
            h.scale = scale;
            h.color = color;
            h.space = space;
            h.strike = strike16;
            self.glyphs.set_window(r, block.base, a, e, [ox, base]);
        }
        for r in first + n as u32..first + cap {
            self.glyphs.runs[r as usize].len = 0;
        }
        if damage {
            for (i, &span) in spans.iter().enumerate() {
                let same = i < old_n && old_base == Some(block.base) && old_spans_v[i] == span;
                if !same {
                    let Rt { glyphs, bough, .. } = self;
                    glyphs.mark_window(first + i as u32, widths[i], &b, &mut CoreMarks(bough.plane_mut()));
                }
            }
        }
        let t = &mut self.texts[ti as usize];
        t.ink = ink;
        if let TShape::Lines { first: f, cap: c, n: nn, widths: w, spans: s, block: bl, .. } = &mut t.shape {
            *f = first;
            *c = cap;
            *nn = n as u32;
            *w = widths;
            s.clear();
            s.extend_from_slice(spans);
            *bl = Some(block);
        }
    }

    /// Glyph instances (beyond Latin-1, a trigger the token table refuses, or per-glyph colour).
    fn build_glyphs(&mut self, ti: u32) -> u32 {
        self.text.scratch.clear();
        let (strike, size_px, ox, oy, lh, color, space) = {
            let t = &self.texts[ti as usize];
            (t.strike as StrikeId, t.size_px, t.ox, t.oy, t.line_h, t.color, t.space)
        };
        let text = std::mem::take(&mut self.texts[ti as usize].text);
        for (i, line) in text.split('\n').enumerate() {
            self.text.line(strike, size_px, line, ox, (oy + i as f32 * lh).round(), color);
        }
        self.texts[ti as usize].text = text;
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
                    Instance { pos: g.xy, size: g.wh, color: g.color, color2: 0, radius: 0.0, bw: 0.0, uv: g.uv, space, kind: KIND_GLYPH, flags: 0, ext: 0 }
                }
                None => Instance { space, kind: KIND_GLYPH, ..Default::default() },
            };
            self.inst[(first + i as u32) as usize] = ins;
        }
        let ink = if cnt == 0 { NRect::EMPTY } else { self.ink_rect(space, x0, y0, x1 - x0, y1 - y0, 1.0) };
        let t = &mut self.texts[ti as usize];
        t.ink = ink;
        t.shape = TShape::Glyphs { run };
        pay_irun(first, cnt.max(1) as u32)
    }

    /// A text node's value changed. Labels patch (exact tile marks at update time); line windows re-point
    /// with per-line marks; glyph instances are rebuilt (old and new ink damaged).
    pub(crate) fn text_set(&mut self, ti: u32, new: &str) {
        if self.texts[ti as usize].text == new {
            return;
        }
        self.ctr.text_set += 1;
        let h = self.texts[ti as usize].handle;
        let shape = std::mem::take(&mut self.texts[ti as usize].shape);
        match shape {
            TShape::Label(mut lb) if !new.contains('\n') => {
                let old_w = lb.width();
                let ok = {
                    let Rt { glyphs, text: ts, bough, .. } = self;
                    glyphs.set_text(&ts.shaper, &mut ts.cache, &mut lb, new, &mut CoreMarks(bough.plane_mut()))
                };
                if ok {
                    let w = lb.width();
                    let t = &mut self.texts[ti as usize];
                    t.text.clear();
                    t.text.push_str(new);
                    if w != old_w {
                        let (space, ox, oy) = (t.space, t.ox, t.oy);
                        let scale = t.size_px / self.text.shaper.script.upem as f32;
                        let ink = line_ink(&lb.bounds, scale, ox, oy as i32, w, self.glyphs.spaces[space as usize]);
                        self.texts[ti as usize].ink = ink;
                        self.bough.set_ink_handle_nodamage(h, ink);
                    }
                    self.texts[ti as usize].shape = TShape::Label(lb);
                    return;
                }
                self.texts[ti as usize].shape = TShape::Label(lb);
            }
            TShape::Set(mut set) => {
                let strike = self.texts[ti as usize].strike as StrikeId;
                let b = self.text.bounds(strike);
                let r = {
                    let Rt { glyphs, text: ts, bough, .. } = self;
                    set.set_text(glyphs, &ts.shaper, new, &b, Some(&mut CoreMarks(bough.plane_mut())))
                };
                let (first, cap) = (set.first, set.cap);
                self.texts[ti as usize].shape = TShape::Set(set);
                if let Ok(st) = r {
                    self.ctr.lines_shaped += st.shaped as u64;
                    self.ctr.lines_kept += st.kept as u64;
                    if st.exotic {
                        self.ready_set(ti);
                    }
                    if st.moved {
                        self.bough.set_payload_handle(h, pay_runs(first, cap));
                    }
                    let t = &mut self.texts[ti as usize];
                    t.text.clear();
                    t.text.push_str(new);
                    let ink = self.set_ink(ti);
                    if ink != self.texts[ti as usize].ink {
                        self.texts[ti as usize].ink = ink;
                        self.bough.set_ink_handle_nodamage(h, ink);
                    }
                    return;
                }
            }
            other => self.texts[ti as usize].shape = other,
        }
        // representation change (or glyph instances): rebuild, damage old and new ink
        let old_ink = self.texts[ti as usize].ink;
        let old = std::mem::take(&mut self.texts[ti as usize].shape);
        release(self, TextNode { shape: old, ..Default::default() });
        {
            let t = &mut self.texts[ti as usize];
            t.text.clear();
            t.text.push_str(new);
        }
        let payload = self.text_build(ti);
        self.bough.set_payload_handle(h, payload);
        let new_ink = self.texts[ti as usize].ink;
        self.bough.damage_rect(old_ink);
        self.bough.set_ink_handle(h, new_ink);
    }
}
