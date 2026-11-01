//! Text services: bough-text's SimpleScript shaper + atlas + fused shape-to-instances, with a
//! rustybuzz fallback (in this adapter, not in bough-text) for strings that touch a trigger
//! (`ff`/`fi`/`fl` ligatures) or leave ASCII.
use bough_text::atlas::{GlyphCache, Instance, StrikeId};
use bough_text::{GlyphRun, Shaper, SimpleScript};

pub struct TextSys {
    pub shaper: Shaper,
    pub cache: GlyphCache,
    pub scratch: Vec<Instance>,
    face: rustybuzz::Face<'static>,
    /// Font-unit vertical metrics (hhea).
    pub ascent: f32,
    pub descent: f32,
    pub upem: f32,
    strikes: Vec<(u32, StrikeId)>,
    pub fallbacks: u64,
    run: GlyphRun,
}

impl TextSys {
    pub fn new(font: &[u8]) -> TextSys {
        let script = SimpleScript::build(font, 0).expect("font has no simple-script table");
        let leaked: &'static [u8] = Box::leak(font.to_vec().into_boxed_slice());
        let tf = ttf_parser::Face::parse(leaked, 0).expect("font");
        let (ascent, descent, upem) = (tf.ascender() as f32, -(tf.descender() as f32), tf.units_per_em() as f32);
        TextSys {
            shaper: Shaper::new(script),
            cache: GlyphCache::new(font),
            scratch: Vec::with_capacity(1024),
            face: rustybuzz::Face::from_slice(leaked, 0).expect("rustybuzz face"),
            ascent,
            descent,
            upem,
            strikes: Vec::new(),
            fallbacks: 0,
            run: GlyphRun::default(),
        }
    }

    /// Strike id for a pixel size (cached by the size's bits).
    pub fn strike(&mut self, size_px: f32) -> StrikeId {
        let key = size_px.to_bits();
        if let Some(&(_, s)) = self.strikes.iter().find(|(k, _)| *k == key) {
            return s;
        }
        let s = self.cache.strike(size_px);
        self.strikes.push((key, s));
        s
    }

    /// Baseline offset below the top of a line box of height `lh` (CSS half-leading), px.
    pub fn baseline(&self, size_px: f32, lh_px: f32) -> f32 {
        let content = (self.ascent + self.descent) / self.upem * size_px;
        (lh_px - content) * 0.5 + self.ascent / self.upem * size_px
    }

    /// Append the glyph instances of one line (pen origin `ox`, baseline `oy`, px) to `scratch`.
    pub fn line(&mut self, strike: StrikeId, size_px: f32, line: &str, ox: f32, oy: f32, color: u32) -> usize {
        if let Some(n) = self.cache.shape_instances_ascii(&self.shaper, strike, line, ox, oy, color, &mut self.scratch) {
            return n;
        }
        self.fallbacks += 1;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(line);
        buf.guess_segment_properties();
        let out = rustybuzz::shape(&self.face, &[], buf);
        let (infos, pos) = (out.glyph_infos(), out.glyph_positions());
        let r = &mut self.run;
        r.gid.clear();
        r.x.clear();
        r.cluster.clear();
        r.identity_clusters = true;
        r.upem = self.upem as u16;
        r.size = size_px;
        let mut x = 0i32;
        for (i, p) in infos.iter().zip(pos) {
            r.gid.push(i.glyph_id as u16);
            r.x.push(x + p.x_offset);
            x += p.x_advance;
        }
        r.width = x;
        let run = std::mem::take(&mut self.run);
        let n = self.cache.instances(strike, &run, ox, oy, color, &mut self.scratch);
        self.run = run;
        n
    }

    /// Shape `text` into `out` (font units): the table shaper when it covers the string, else rustybuzz
    /// (x advances and glyph ids only, as in `line`). Used by layout, which needs the run itself
    /// (widths, wrap points) before it needs instances.
    pub fn shape_run(&mut self, text: &str, size_px: f32, out: &mut GlyphRun) {
        if self.shaper.shape_into(text, size_px, out) {
            return;
        }
        self.fallbacks += 1;
        let mut buf = rustybuzz::UnicodeBuffer::new();
        buf.push_str(text);
        buf.guess_segment_properties();
        let shaped = rustybuzz::shape(&self.face, &[], buf);
        out.gid.clear();
        out.x.clear();
        out.cluster.clear();
        out.identity_clusters = true;
        out.upem = self.upem as u16;
        out.size = size_px;
        let mut x = 0i32;
        for (i, p) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
            out.gid.push(i.glyph_id as u16);
            out.x.push(x + p.x_offset);
            x += p.x_advance;
        }
        out.width = x;
    }
}
