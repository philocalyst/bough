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
use bough_core::nrect::WRect;
use bough_core::Handle;
use bough_frame::{pay_irun, pay_runs};
use bough_frame::Instance;
use bough_text::atlas::{GlyphCache, Instance as TInstance, LineBounds, StrikeId, TokStrike};
use bough_text::pool::{pen_px, Label};
use bough_text::lines::{LineSet, LineStyle};
use bough_text::window::Shaped;
use bough_text::{GlyphRun, Shaper, SimpleScript};

/// A face's shaping state while another face is current ([`TextSys::select`]).
struct FaceState {
    shaper: std::sync::Arc<Shaper>,
    face: rustybuzz::Face<'static>,
    ascent: f32,
    descent: f32,
    upem: f32,
    chunks: Vec<(String, Vec<u16>, Vec<i32>, i32)>,
}

impl FaceState {
    fn new(font: &[u8]) -> FaceState {
        let (leaked, shaper) = font_tables(font);
        let tf = ttf_parser::Face::parse(leaked, 0).expect("font");
        FaceState {
            shaper,
            face: rustybuzz::Face::from_slice(leaked, 0).expect("rustybuzz face"),
            ascent: tf.ascender() as f32,
            descent: -(tf.descender() as f32),
            upem: tf.units_per_em() as f32,
            chunks: Vec::new(),
        }
    }
}

/// A face not current: built (`Ready`) or only its font so far (`Pending`: built at first use, or taken
/// from [`prepare_faces`]'s thread).
enum Parked {
    Ready(FaceState),
    Pending(Vec<u8>),
}

type FaceJob = (Vec<u8>, std::thread::JoinHandle<(FaceState, u64)>);
static PRE_FACES: std::sync::Mutex<Vec<FaceJob>> = std::sync::Mutex::new(Vec::new());

/// Start building the shaping state of `faces` (their simple-script tables) on a thread each, from process
/// entry; a face's first use takes it.
pub fn prepare_faces(faces: &[(u8, Vec<u8>)]) {
    let mut jobs = PRE_FACES.lock().unwrap();
    for (_, font) in faces {
        let f = font.clone();
        let h = std::thread::Builder::new()
            .name("bough-face".into())
            .spawn(move || {
                let i0 = bough_sys::perf::thread_instructions();
                let st = FaceState::new(&f);
                (st, bough_sys::perf::thread_instructions() - i0)
            })
            .expect("face thread");
        jobs.push((font.clone(), h));
    }
}

/// Instructions the face threads retired (for the faces taken so far).
pub static FACE_PREP_INSTR: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn take_face(font: &[u8]) -> FaceState {
    let job = {
        let mut jobs = PRE_FACES.lock().unwrap();
        jobs.iter().position(|j| j.0.len() == font.len() && j.0[..font.len().min(4096)] == font[..font.len().min(4096)]).map(|i| jobs.swap_remove(i))
    };
    match job {
        Some((_, h)) => {
            let (st, n) = h.join().expect("face thread");
            FACE_PREP_INSTR.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
            st
        }
        None => FaceState::new(font),
    }
}

// SAFETY: a FaceState is plain tables plus a rustybuzz face over leaked ('static) bytes; it moves from the
// thread that built it to the one that uses it, never shared.
unsafe impl Send for FaceState {}

/// Logical faces a document names (`LNode::face`): regular, bold, mono, oblique.
pub const FACE_REGULAR: u8 = 0;
pub const FACE_BOLD: u8 = 1;
pub const FACE_MONO: u8 = 2;
pub const FACE_OBLIQUE: u8 = 3;

pub struct TextSys {
    /// The regular face's bytes (kept for the process, shared with the shaper's face).
    font: &'static [u8],
    /// The current face's shaping tables (face 0 between operations; [`TextSys::select`]): immutable, built
    /// once per font per process and shared by every window (`font_tables`), as a renderer's font system is.
    pub shaper: std::sync::Arc<Shaper>,
    pub cache: GlyphCache,
    pub scratch: Vec<TInstance>,
    face: rustybuzz::Face<'static>,
    /// Font-unit vertical metrics (hhea).
    pub ascent: f32,
    pub descent: f32,
    pub upem: f32,
    strikes: Vec<(u64, StrikeId)>,
    /// Lines that needed rustybuzz for a non-ASCII chunk (segmented) or for the whole line.
    pub segmented: u64,
    pub fallbacks: u64,
    run: GlyphRun,
    seg: GlyphRun,
    /// Shaped non-Latin-1 chunks (gids, x offsets, width in font units), by text.
    chunks: Vec<(String, Vec<u16>, Vec<i32>, i32)>,
    /// Scratch for multi-line shaping (newlines as spaces).
    pub bytes: Vec<u8>,
    /// The current face, the others' parked state (`alt[f]`: `None` for the current one) and the logical
    /// face -> face id map (absent faces fall back to 0).
    cur: u8,
    alt: Vec<Option<Parked>>,
    logical: [u8; 4],
}

/// The process-wide font table cache: (FNV-1a of the font bytes, the bytes kept for the faces, the shaper).
#[allow(clippy::type_complexity)]
static FONTS: std::sync::Mutex<Vec<(u64, &'static [u8], std::sync::Arc<Shaper>)>> = std::sync::Mutex::new(Vec::new());

/// The font's bytes (kept for the process: the faces borrow them) and its shaping tables, built on first use.
fn font_tables(font: &[u8]) -> (&'static [u8], std::sync::Arc<Shaper>) {
    // A known font is found by comparing bytes (one vectorised `memcmp` against the few fonts of the same
    // length), never by hashing the file: the byte-at-a-time hash this replaced cost ~3 instructions per
    // byte of the font on every `App::new` (round 6: the largest single item of a small app's start).
    let mut fonts = FONTS.lock().unwrap();
    if let Some((_, b, s)) = fonts.iter().find(|f| f.1.len() == font.len() && f.1 == font) {
        return (b, s.clone());
    }
    let h = 0;
    // built once per process here, and once per font ever through bough-text's file cache
    let script = SimpleScript::build_cached(font, 0).expect("font has no simple-script table");
    let leaked: &'static [u8] = Box::leak(font.to_vec().into_boxed_slice());
    let s = std::sync::Arc::new(Shaper::new(script));
    fonts.push((h, leaked, s.clone()));
    (leaked, s)
}

/// Bytes of a shaper's tables (its struct, the class-pair matrix, the token steps and ligatures; the dense
/// ASCII table is 128 x 128 u16 = 32 KiB, `Shaper::dense`).
fn shaper_bytes(s: &Shaper) -> usize {
    let sc = &s.script;
    let tok = sc.tokens.as_ref().map_or(0, |t| t.step.len() * 2 + t.ligs.iter().map(|l| l.1.capacity() + 24).sum::<usize>());
    size_of::<Shaper>() + sc.matrix.capacity() * 2 + tok + 128 * 128 * 2
}

/// Process-wide text memory (font bytes kept for the faces and the shaping tables), as ledger lines.
pub fn fonts_ledger(l: &mut bough_mem::Ledger) {
    use bough_mem::Scales::Fixed;
    let fonts = FONTS.lock().unwrap();
    let bytes: usize = fonts.iter().map(|f| f.1.len()).sum();
    l.many("text", format!("font bytes kept for the faces ({} fonts, leaked)", fonts.len()), Fixed, bytes, bytes);
    let tables: usize = fonts.iter().map(|f| shaper_bytes(&f.2)).sum();
    l.many("text", "shaping tables (SimpleScript + Shaper, per font)", Fixed, tables, tables);
}

impl TextSys {
    /// The regular face's bytes (process-lifetime).
    pub fn font_bytes(&self) -> &'static [u8] {
        self.font
    }

    /// This window's text memory (the glyph cache and the shaper's scratch), as ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::{Content, Fixed};
        self.cache.ledger(l);
        l.vec("text", "instance scratch", &self.scratch, self.scratch.capacity(), Content);
        let ch = |c: &Vec<(String, Vec<u16>, Vec<i32>, i32)>| c.capacity() * 80 + c.iter().map(|x| x.0.capacity() + x.1.capacity() * 2 + x.2.capacity() * 4).sum::<usize>();
        let run = |r: &GlyphRun| r.gid.capacity() * 2 + r.x.capacity() * 4 + r.cluster.capacity() * 4;
        let mut b = ch(&self.chunks) + run(&self.run) + run(&self.seg) + self.bytes.capacity() + self.strikes.capacity() * 16;
        for p in self.alt.iter().flatten() {
            b += match p {
                Parked::Pending(f) => f.capacity(),
                Parked::Ready(fs) => ch(&fs.chunks),
            };
        }
        l.many("text", "shaper scratch, chunk cache, parked faces", Fixed, b, b);
    }
    /// `toks`: the (carved, GPU-visible) column of strike token tables.
    pub fn new(font: &[u8], toks: bough_mem::Col<TokStrike>) -> TextSys {
        let (leaked, shaper) = font_tables(font);
        let tf = ttf_parser::Face::parse(leaked, 0).expect("font");
        let (ascent, descent, upem) = (tf.ascender() as f32, -(tf.descender() as f32), tf.units_per_em() as f32);
        TextSys {
            font: leaked,
            shaper,
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
            cur: 0,
            alt: vec![None],
            logical: [0; 4],
        }
    }

    /// Add `font` as logical face `logical` (FACE_BOLD, FACE_MONO, ...): its own shaper, metrics and strikes.
    pub fn add_face(&mut self, logical: u8, font: &[u8]) {
        let id = self.cache.add_face(font);
        debug_assert_eq!(id as usize, self.alt.len());
        self.alt.push(Some(Parked::Pending(font.to_vec())));
        self.logical[logical as usize & 3] = id;
    }

    /// The face id of logical face `logical` (0 when the document's font set lacks it).
    pub fn face_id(&self, logical: u8) -> u8 {
        self.logical[logical as usize & 3]
    }

    /// Make face `f` current: its shaper, rustybuzz face and metrics move into the fields every shaping
    /// path reads (boxes and a few words: no table is copied). Operations on another face's text select it
    /// and select 0 again when done.
    pub fn select(&mut self, f: u8) {
        if f == self.cur {
            return;
        }
        let mut st = match self.alt[f as usize].take().expect("face") {
            Parked::Ready(s) => s,
            Parked::Pending(font) => take_face(&font),
        };
        std::mem::swap(&mut self.shaper, &mut st.shaper);
        std::mem::swap(&mut self.face, &mut st.face);
        std::mem::swap(&mut self.ascent, &mut st.ascent);
        std::mem::swap(&mut self.descent, &mut st.descent);
        std::mem::swap(&mut self.upem, &mut st.upem);
        std::mem::swap(&mut self.chunks, &mut st.chunks);
        self.alt[self.cur as usize] = Some(Parked::Ready(st));
        self.cur = f;
    }

    /// Select the face of `strike`.
    fn select_strike(&mut self, strike: StrikeId) {
        let f = self.cache.strike_face(strike);
        self.select(f);
    }

    /// Strike id for a pixel size of face 0 (cached by the size's bits).
    pub fn strike(&mut self, size_px: f32) -> StrikeId {
        self.strike_in(0, size_px)
    }

    /// Strike id for a pixel size of face id `face`.
    pub fn strike_in(&mut self, face: u8, size_px: f32) -> StrikeId {
        let key = size_px.to_bits() as u64 | (face as u64) << 32;
        if let Some(&(_, s)) = self.strikes.iter().find(|(k, _)| *k == key) {
            return s;
        }
        let s = self.cache.strike_in(face, size_px);
        // Glyphs rasterised from process entry (`bough::app::prepare`), if this size was named there.
        self.cache.absorb_prerendered(s);
        // ASCII-complete from the start (every path may draw any printable byte without a readiness check;
        // text round 3). A face without the token model draws glyph instances (entries rasterised on use).
        self.select(face);
        if let Some(tt) = self.shaper.script.tokens.as_ref() {
            self.cache.prepare_tokens(s, &tt.gid, 0x20..0x7F);
        }
        self.select(0);
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

    /// [`baseline`](Self::baseline) in the face of `strike`.
    pub fn baseline_of(&mut self, strike: StrikeId, size_px: f32, lh_px: f32) -> f32 {
        self.select_strike(strike);
        let b = self.baseline(size_px, lh_px);
        self.select(0);
        b
    }

    /// The strike's conservative line bounds (O(1) line damage and record ink).
    pub fn bounds(&mut self, strike: StrikeId) -> LineBounds {
        self.select_strike(strike);
        let tt = self.shaper.script.tokens.as_ref().expect("token model");
        let b = self.cache.line_bounds(strike, tt, self.shaper.script.upem);
        self.select(0);
        b
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
        self.select_strike(strike);
        let n = self.line_cur(strike, size_px, line, ox, oy, color);
        self.select(0);
        n
    }

    fn line_cur(&mut self, strike: StrikeId, size_px: f32, line: &str, ox: f32, oy: f32, color: u32) -> usize {
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
    pub ink: WRect,
    pub shape: TShape,
    /// Per-glyph colour (instance representation even for covered text).
    pub per_glyph: bool,
    /// The format of a numeric operand (`sig!(..).num(f, fmt)`).
    pub num: Option<bough_text::numeric::NumFmt>,
}

impl TextNode {
    /// The node's current text. A label holds its own bytes (its patch compares against them), so its `text`
    /// string is not kept current on the label path (text round 3); read the text through this.
    pub fn bytes(&self) -> &[u8] {
        match &self.shape {
            TShape::Label(lb) => lb.text(),
            _ => self.text.as_bytes(),
        }
    }
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
fn line_ink(b: &LineBounds, scale: f32, ox: f32, oy: i32, w: i32, sp: [i32; 2]) -> WRect {
    if w <= 0 {
        return WRect::EMPTY;
    }
    let oxh = ox + 0.5;
    WRect::new(pen_px(0, scale, oxh) + sp[0] + b.left, oy + sp[1] + b.top, pen_px(w, scale, oxh) + sp[0] + b.over, oy + sp[1] + b.bottom)
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
    fn set_ink(&mut self, ti: u32) -> WRect {
        let strike = self.texts[ti as usize].strike as StrikeId;
        let b = self.text.bounds(strike);
        let TShape::Set(set) = &self.texts[ti as usize].shape else { unreachable!() };
        let st = set.style;
        let sp = self.glyphs.spaces[st.space as usize];
        let mut ink = WRect::EMPTY;
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
        let mut ink = WRect::EMPTY;
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
        let ink = if cnt == 0 { WRect::EMPTY } else { self.ink_rect(space, x0, y0, x1 - x0, y1 - y0, 1.0) };
        let t = &mut self.texts[ti as usize];
        t.ink = ink;
        t.shape = TShape::Glyphs { run };
        pay_irun(first, cnt.max(1) as u32)
    }

    /// A text node's value changed. Labels patch (exact tile marks at update time); line windows re-point
    /// with per-line marks; glyph instances are rebuilt (old and new ink damaged).
    pub(crate) fn text_set(&mut self, ti: u32, new: &str) {
        // A label holds its own text (its 16-byte head and its bytes) and its patch starts with the comparison
        // of old and new: the node's `String` is neither compared nor copied on this path (the value was there
        // all along); it is refreshed only on a change of representation. A `\n` or an uncovered codepoint makes
        // the label refuse (a control byte never tokenizes), which is that change.
        {
            let Rt { glyphs, text: ts, bough, texts, ctr, .. } = self;
            let t = &mut texts[ti as usize];
            if let TShape::Label(lb) = &mut t.shape {
                ctr.text_set += 1;
                let old_w = lb.width();
                if glyphs.set_text(&ts.shaper, &mut ts.cache, lb, new, &mut CoreMarks(bough.plane_mut())) {
                    let w = lb.width();
                    if w != old_w {
                        let scale = t.size_px / ts.shaper.script.upem as f32;
                        let ink = line_ink(&lb.bounds, scale, t.ox, t.oy as i32, w, glyphs.spaces[t.space as usize]);
                        t.ink = ink;
                        bough.set_ink_handle_nodamage_wide(t.handle, ink);
                    }
                    return;
                }
            } else if t.text == new {
                return;
            } else {
                ctr.text_set += 1;
            }
        }
        let h = self.texts[ti as usize].handle;
        let shape = std::mem::take(&mut self.texts[ti as usize].shape);
        match shape {
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
                        self.bough.set_ink_handle_nodamage_wide(h, ink);
                    }
                    return;
                }
            }
            other => self.texts[ti as usize].shape = other,
        }
        // representation change (or glyph instances): rebuild, damage old (the record's current ink: a move
        // translates records, not this cache) and new ink
        let old_ink = self.cur_ink(h);
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
        self.bough.set_ink_handle_wide(h, new_ink);
    }
}
