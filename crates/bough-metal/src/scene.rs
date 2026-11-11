//! Dashboard-like scene living directly in the shared pool buffer, plus CPU-side `ink` bounds
//! (absolute device pixels, clip-intersected, +1px AA margin) used for culling and damage.

use crate::damage::Plane;
use crate::instance::*;

pub const GLYPH_W: u16 = 14;
pub const GLYPH_H: u16 = 24;
pub const ATLAS_W: usize = 1024;
pub const ATLAS_H: usize = 128;
pub const GLYPHS: usize = 96;

/// IRect -> bough-core NRect lanes (x0, y0, -x1, -y1); empty -> canonical EMPTY (i16::MAX x 4).
#[inline]
pub fn nrect(r: IRect) -> [i16; 4] {
    if r.is_empty() { [i16::MAX; 4] } else { [r.x0 as i16, r.y0 as i16, (-r.x1) as i16, (-r.y1) as i16] }
}

pub struct Rng(pub u64);
impl Rng {
    #[inline]
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    #[inline]
    pub fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 33) as usize) % n
    }
    #[inline]
    pub fn color(&mut self) -> u32 {
        let v = self.next();
        rgba((v >> 8) as u8 | 0x20, (v >> 24) as u8 | 0x20, (v >> 40) as u8 | 0x20, 255)
    }
}

/// Dummy atlas: pseudo-glyph bitmaps, R8.
pub fn make_atlas() -> Vec<u8> {
    let mut a = vec![0u8; ATLAS_W * ATLAS_H];
    let mut rng = Rng(0x9E3779B97F4A7C15);
    let cols = ATLAS_W / GLYPH_W as usize;
    for g in 0..GLYPHS {
        let (ox, oy) = ((g % cols) * GLYPH_W as usize, (g / cols) * GLYPH_H as usize);
        // 7x12 blocks of 2x2 px with a stroke-like fill
        for by in 0..12 {
            for bx in 0..7 {
                let on = (rng.next() >> 40) & 3 != 0 && bx > 0 && bx < 6 && by > 1 && by < 11;
                if on {
                    for dy in 0..2 {
                        for dx in 0..2 {
                            a[(oy + by * 2 + dy) * ATLAS_W + ox + bx * 2 + dx] = 255;
                        }
                    }
                }
            }
        }
    }
    a
}

pub struct Scene {
    pool: *mut u8,
    pub n: usize,
    pub ink: Vec<IRect>,
    /// union of `ink` per 64-instruction page (the Bough's directory: cull skips whole pages)
    pub page_ink: Vec<IRect>,
    pub space_run: Vec<(usize, usize)>, // instance run per space (contiguous), empty = (0,0)
    pub n_spaces: usize,
    pub small: Vec<u32>,  // indices of small instances (glyphs and little rects): colour-change targets
    pub cards: Vec<u16>,  // movable spaces
    pub table_space: u16,
    pub table_clip: IRect,
    pub bg_inst: u32,
    pub acc: Plane,
    /// Content-addressed damage (Zobrist): per 16x16 tile, XOR of a 64-bit hash of the full drawing state of every
    /// instance whose ink touches the tile. Two scene states with equal tile hash draw identical tile pixels
    /// (up to a 2^-64 collision). Empty = off.
    pub zob: Vec<u64>,
    /// bumped by `invalidate_all` (atlas/global change): a surface drawn under another salt matches nothing.
    pub salt: u64,
    zseed: u64,
    /// BOUGH_HEAVYTEXT=1: + 84 numeric labels (in the cards) + a 5-line log pane (windows over a ring)
    pub heavy_text: bool,
    /// BOUGH_RUNS=1: the GPU mirror holds text as RUN records (bough-text format) instead of glyph instances
    pub gpu_runs: bool,
    /// every text string as a bough-text run: (first glyph instance, len); slot base = run's header.base
    pub runs: Vec<(usize, usize)>,
    pub inst_run: Vec<u32>,
    n_slots: usize,
    /// mirror record list (paint order): instance index, or run index | RUN_BIT; and instance -> record
    pub recs: Vec<u32>,
    pub inst_rec: Vec<u32>,
    pub numeric: Vec<usize>,
    pub log_runs: Vec<usize>,
    log_head: usize,
    log_base: usize,
    pub log_rect: IRect,
    pub scroll_y: f32,
    pub dirty_count: u64,
}

impl Scene {
    pub fn new(pool: *mut u8) -> Self {
        let mut s = Scene {
            pool,
            n: 0,
            ink: Vec::with_capacity(MAX_INST),
            page_ink: vec![IRect::EMPTY; MAX_INST / 64],
            space_run: vec![(0, 0); MAX_SPACES],
            n_spaces: 1,
            small: vec![],
            cards: vec![],
            table_space: 0,
            table_clip: IRect::EMPTY,
            bg_inst: 0,
            acc: Plane::new(),
            zob: Vec::new(),
            salt: 1,
            zseed: 0x1234_5678,
            heavy_text: std::env::var_os("BOUGH_HEAVYTEXT").is_some(),
            gpu_runs: std::env::var_os("BOUGH_RUNS").is_some(),
            runs: vec![],
            inst_run: vec![],
            n_slots: 0,
            recs: vec![],
            inst_rec: vec![],
            numeric: vec![],
            log_runs: vec![],
            log_head: 0,
            log_base: 0,
            log_rect: IRect::EMPTY,
            scroll_y: 0.0,
            dirty_count: 0,
        };
        s.write_header();
        s.build();
        s.rebuild_mirror();
        s
    }

    fn write_header(&mut self) {
        let h = Hdr {
            viewport: [W as f32, H as f32],
            spaces_off: SPACES_OFF as u32,
            clips_off: CLIPS_OFF as u32,
            rects_off: RECTS_OFF as u32,
            inst_off: INST_OFF as u32,
            list_off: LIST_OFF as u32,
            n_inst: 0,
            plane_off: PLANE_OFF as u32,
            pages_off: PAGES_OFF as u32,
            dir_page_off: DIR_PAGE_OFF as u32,
            dir_ink_off: DIR_INK_OFF as u32,
            n_dir: 0,
            pagepool_off: PAGEPOOL_OFF as u32,
            debug: 0,
            stats_off: STATS_OFF as u32,
            gx_off: GX_OFF as u32,
            gent_off: GENT_OFF as u32,
            runs_off: RUNS_OFF as u32,
            entries_off: ENTRIES_OFF as u32,
            space_clip_off: SPACE_CLIP_OFF as u32,
        };
        unsafe { (self.pool as *mut Hdr).write(h) };
        for s in 0..MAX_SPACES {
            self.set_space_raw(s, [0.0, 0.0]);
        }
        for c in 0..MAX_CLIPS {
            self.set_clip(c, IRect::FULL);
        }
        // one strike: entry id = glyph token; procedural atlas glyphs 0..96
        let cols = ATLAS_W / GLYPH_W as usize;
        for g in 0..N_ENTRIES {
            let e = if g < GLYPHS {
                let (gx, gy) = ((g % cols) as u16 * GLYPH_W, (g / cols) as u16 * GLYPH_H);
                Entry { geo: [0.0, 0.0, GLYPH_W as f32, GLYPH_H as f32], uv: [gx, gy, GLYPH_W, GLYPH_H], state: 1, pad: 0 }
            } else {
                Entry::default()
            };
            unsafe { *(self.pool.add(ENTRIES_OFF) as *mut Entry).add(g) = e };
        }
        for sp in 0..MAX_SPACES {
            unsafe { *(self.pool.add(SPACE_CLIP_OFF) as *mut u32).add(sp) = 0 };
        }
        // core-layout page pool: every slot starts as the padding sentinel (depth 0xFFFF, ink EMPTY, rest 0)
        for pg in 0..MAX_DIR {
            unsafe {
                let b = self.pool.add(PAGEPOOL_OFF + pg * PG_BYTES);
                std::ptr::write_bytes(b, 0, PG_BYTES);
                for k in 0..64 {
                    *(b as *mut u16).add(k) = 0xFFFF;
                    *(b.add(PG_INK) as *mut [i16; 4]).add(k) = [i16::MAX; 4];
                }
                for g in 0..8 {
                    *(b.add(PG_GINK) as *mut [i16; 4]).add(g) = [i16::MAX; 4];
                }
                *(self.pool.add(DIR_PAGE_OFF) as *mut u32).add(pg) = pg as u32;
                *(self.pool.add(DIR_INK_OFF) as *mut [i16; 4]).add(pg) = [i16::MAX; 4];
            }
        }
        for pg in 0..MAX_INST / 64 {
            unsafe { *((self.pool.add(PAGES_OFF) as *mut [i32; 4]).add(pg)) = [1, 1, 0, 0] };
        }
    }

    #[inline]
    pub fn inst(&self, i: usize) -> &mut Instance {
        debug_assert!(i < MAX_INST);
        unsafe { &mut *((self.pool.add(INST_OFF) as *mut Instance).add(i)) }
    }
    #[inline]
    pub fn space(&self, s: usize) -> [f32; 2] {
        unsafe { *((self.pool.add(SPACES_OFF) as *const [f32; 2]).add(s)) }
    }
    #[inline]
    fn set_space_raw(&self, s: usize, v: [f32; 2]) {
        unsafe { *((self.pool.add(SPACES_OFF) as *mut [f32; 2]).add(s)) = v };
    }
    pub fn clip(&self, c: usize) -> IRect {
        let f = unsafe { *((self.pool.add(CLIPS_OFF) as *const [f32; 4]).add(c)) };
        IRect::new(f[0] as i32, f[1] as i32, f[2] as i32, f[3] as i32)
    }
    fn set_clip(&self, c: usize, r: IRect) {
        unsafe { *((self.pool.add(CLIPS_OFF) as *mut [f32; 4]).add(c)) = [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32] };
    }
    pub fn list_ptr(&self) -> *mut u32 {
        unsafe { self.pool.add(LIST_OFF) as *mut u32 }
    }
    pub fn rects_ptr(&self) -> *mut [f32; 4] {
        unsafe { self.pool.add(RECTS_OFF) as *mut [f32; 4] }
    }

    fn compute_ink(&self, i: usize) -> IRect {
        let it = self.inst(i);
        let off = self.space(it.space as usize);
        let x0 = (it.pos[0] + off[0]).floor() as i32 - 1;
        let y0 = (it.pos[1] + off[1]).floor() as i32 - 1;
        let x1 = (it.pos[0] + off[0] + it.size[0]).ceil() as i32 + 1;
        let y1 = (it.pos[1] + off[1] + it.size[1]).ceil() as i32 + 1;
        IRect::new(x0, y0, x1, y1).intersect(&self.clip(it.ext as usize)).intersect(&IRect::FULL)
    }

    fn push(&mut self, it: Instance) -> usize {
        let i = self.n;
        *self.inst(i) = it;
        self.n += 1;
        unsafe { *(self.pool.add(HDR_N_INST_OFF) as *mut u32) = self.n as u32 };
        let ink = self.compute_ink(i);
        self.ink.push(ink);
        let pg = &mut self.page_ink[i / 64];
        *pg = pg.union(&ink);
        let v = if pg.is_empty() { [1, 1, 0, 0] } else { [pg.x0, pg.y0, pg.x1, pg.y1] };
        unsafe { *((self.pool.add(PAGES_OFF) as *mut [i32; 4]).add(i / 64)) = v };
        self.inst_run.push(u32::MAX);
        i
    }

    /// Build the core-layout mirror (records in paint order) from scratch. Instance mode: one record per instance.
    /// Runs mode: a text run's glyph instances collapse into ONE record (kind RUN, payload = run | RUN_BIT).
    pub fn rebuild_mirror(&mut self) {
        self.recs.clear();
        self.inst_rec = vec![0; self.n];
        let mut i = 0;
        while i < self.n {
            let r = self.inst_run[i];
            if self.gpu_runs && r != u32::MAX {
                let (first, len) = self.runs[r as usize];
                debug_assert_eq!(first, i);
                for k in i..i + len {
                    self.inst_rec[k] = self.recs.len() as u32;
                }
                self.recs.push(r | RUN_BIT);
                i += len;
            } else {
                self.inst_rec[i] = self.recs.len() as u32;
                self.recs.push(i as u32);
                i += 1;
            }
        }
        assert!(self.recs.len() <= MAX_DIR * 64);
        for (r, &p) in self.recs.iter().enumerate() {
            unsafe {
                let b = self.pool.add(PAGEPOOL_OFF + (r / 64) * PG_BYTES);
                *(b as *mut u16).add(r % 64) = 0;
                *b.add(PG_KIND + r % 64) = if p & RUN_BIT != 0 { KIND_RUN } else { self.inst(p as usize).kind };
                *(b.add(PG_PAYLOAD) as *mut u32).add(r % 64) = p;
            }
        }
        unsafe { *(self.pool.add(HDR_N_DIR_OFF) as *mut u32) = (self.recs.len() as u32 + 63) / 64 };
        self.refresh_recs(0, self.recs.len());
    }

    /// Ink of mirror record r: an instance's ink, or the union of a run's glyph inks.
    #[inline]
    fn rec_ink(&self, r: usize) -> [i16; 4] {
        let p = self.recs[r];
        if p & RUN_BIT == 0 {
            return nrect(self.ink[p as usize]);
        }
        let (first, len) = self.runs[(p & !RUN_BIT) as usize];
        let mut u = [i16::MAX; 4];
        for k in first..first + len {
            let v = nrect(self.ink[k]);
            for l in 0..4 {
                u[l] = u[l].min(v[l]);
            }
        }
        u
    }

    fn rect(&mut self, space: u16, x: f32, y: f32, w: f32, h: f32, color: u32) -> usize {
        self.push(Instance { pos: [x, y], size: [w, h], color, space, kind: KIND_SOLID, ..Default::default() })
    }
    fn rrect(&mut self, space: u16, x: f32, y: f32, w: f32, h: f32, r: f32, color: u32) -> usize {
        self.push(Instance { pos: [x, y], size: [w, h], color, radius: r, space, kind: KIND_RRECT, ..Default::default() })
    }
    fn border(&mut self, space: u16, x: f32, y: f32, w: f32, h: f32, r: f32, bw: f32, bc: u32, fill: u32) -> usize {
        self.push(Instance { pos: [x, y], size: [w, h], color: bc, color2: fill, radius: r, bw, space, kind: KIND_BORDER, ..Default::default() })
    }
    fn text(&mut self, space: u16, x: f32, y: f32, s: &[u8], color: u32, clip: u32, rng: &mut Rng) -> usize {
        let toks: Vec<usize> = s.iter().map(|_| (rng.next() as usize >> 20) % GLYPHS).collect();
        self.text_tokens(space, x, y, &toks, color, clip)
    }

    /// One string = glyph instances (render paths) AND a bough-text run: slots x = 15 px pen steps in font units
    /// (64 per px, scale 1/64), ent = token; floor(fma(float(x - x0), scale, ox + 0.5)) + geo == x + 15k exactly.
    fn text_tokens(&mut self, space: u16, x: f32, y: f32, toks: &[usize], color: u32, clip: u32) -> usize {
        let cols = ATLAS_W / GLYPH_W as usize;
        let run = self.runs.len();
        let first = self.n;
        let base = self.n_slots;
        assert!(base + toks.len() <= MAX_GLYPHS && run < MAX_RUNS);
        for (k, &g) in toks.iter().enumerate() {
            let (gx, gy) = ((g % cols) as u16 * GLYPH_W, (g / cols) as u16 * GLYPH_H);
            let i = self.push(Instance {
                pos: [x + k as f32 * 15.0, y],
                size: [GLYPH_W as f32, GLYPH_H as f32],
                color,
                uv: [gx, gy, GLYPH_W, GLYPH_H],
                space,
                kind: KIND_GLYPH,
                ext: clip,
                ..Default::default()
            });
            self.small.push(i as u32);
            self.inst_run[i] = run as u32;
            unsafe {
                *(self.pool.add(GX_OFF) as *mut i32).add(base + k) = (k * 15 * 64) as i32;
                *(self.pool.add(GENT_OFF) as *mut u16).add(base + k) = g as u16;
            }
        }
        self.n_slots += toks.len();
        self.runs.push((first, toks.len()));
        let h = RunHeader { origin: [x, y], scale: 1.0 / 64.0, color, space, strike: 0, base: base as u32, len: toks.len() as u32, x0: 0 };
        unsafe { *(self.pool.add(RUNS_OFF) as *mut RunHeader).add(run) = h };
        if clip != 0 {
            unsafe { *(self.pool.add(SPACE_CLIP_OFF) as *mut u32).add(space as usize) = clip };
        }
        run
    }

    #[inline]
    fn run_header(&self, r: usize) -> &mut RunHeader {
        unsafe { &mut *(self.pool.add(RUNS_OFF) as *mut RunHeader).add(r) }
    }
    #[inline]
    fn entry_uv(&self, g: usize) -> [u16; 4] {
        unsafe { (*(self.pool.add(ENTRIES_OFF) as *const Entry).add(g)).uv }
    }

    fn begin_space(&mut self) -> u16 {
        let s = self.n_spaces as u16;
        self.n_spaces += 1;
        self.space_run[s as usize].0 = self.n;
        s
    }
    fn end_space(&mut self, s: u16) {
        self.space_run[s as usize].1 = self.n;
    }

    fn build(&mut self) {
        let mut rng = Rng(0xDA5B0A12D);
        let ink_dark = rgba(0x16, 0x18, 0x1d, 255);
        let panel = rgba(0x20, 0x23, 0x2b, 255);
        let text = rgba(0xdd, 0xe1, 0xea, 255);
        let accent = rgba(0x4c, 0x8d, 0xf6, 255);
        // 0 background
        self.bg_inst = self.rect(0, 0.0, 0.0, W as f32, H as f32, ink_dark) as u32;
        // sidebar
        self.rect(0, 0.0, 0.0, 320.0, H as f32, rgba(0x11, 0x12, 0x16, 255));
        for k in 0..16 {
            let y = 140.0 + k as f32 * 64.0;
            self.rrect(0, 20.0, y, 280.0, 52.0, 12.0, if k == 2 { accent } else { rgba(0x1b, 0x1e, 0x25, 255) });
            self.text(0, 44.0, y + 14.0, &[0; 12], text, 0, &mut rng);
        }
        // header
        self.rect(0, 320.0, 0.0, (W - 320) as f32, 120.0, panel);
        self.text(0, 360.0, 48.0, &[0; 20], text, 0, &mut rng);
        for k in 0..4 {
            let x = 1300.0 + k as f32 * 150.0;
            self.rrect(0, x, 34.0, 130.0, 52.0, 26.0, accent);
            self.text(0, x + 24.0, 48.0, &[0; 6], rgba(255, 255, 255, 255), 0, &mut rng);
        }
        // cards: each a movable space
        for k in 0..6 {
            let (cx, cy) = (360.0 + (k % 3) as f32 * 520.0, 150.0 + (k / 3) as f32 * 330.0);
            let s = self.begin_space();
            self.set_space_raw(s as usize, [0.0, 0.0]);
            self.border(s, cx, cy, 480.0, 300.0, 24.0, 3.0, rgba(0x33, 0x38, 0x46, 255), panel);
            self.text(s, cx + 30.0, cy + 26.0, &[0; 10], text, 0, &mut rng);
            self.text(s, cx + 30.0, cy + 70.0, &[0; 6], accent, 0, &mut rng);
            for b in 0..20 {
                let h = 20.0 + (rng.below(120)) as f32;
                let i = self.rect(s, cx + 30.0 + b as f32 * 22.0, cy + 280.0 - h, 16.0, h, accent);
                self.small.push(i as u32);
            }
            if self.heavy_text {
                // 14 numeric labels per card (84 total): 8 glyphs, tabular digits = tokens 16..26
                for l in 0..14 {
                    let (lx, ly) = (cx + 30.0 + (l % 2) as f32 * 220.0, cy + 110.0 + (l / 2) as f32 * 26.0);
                    let toks: Vec<usize> = (0..8).map(|_| 16 + rng.below(10)).collect();
                    let r = self.text_tokens(s, lx, ly, &toks, text, 0);
                    self.numeric.push(r);
                }
            }
            self.end_space(s);
            self.cards.push(s);
        }
        // chart panel (static)
        self.border(0, 360.0, 820.0, 1500.0, 120.0, 20.0, 3.0, rgba(0x33, 0x38, 0x46, 255), panel);
        for b in 0..60 {
            let h = 10.0 + rng.below(80) as f32;
            let i = self.rect(0, 390.0 + b as f32 * 24.0, 930.0 - h, 18.0, h, rgba(0x3f, 0xb9, 0x7a, 255));
            self.small.push(i as u32);
        }
        if self.heavy_text {
            // log pane: 5 lines x 85 glyphs; each line is a run whose header is a WINDOW onto a ring of 5 x 85 slots
            self.log_base = self.n_slots;
            for k in 0..5 {
                let toks: Vec<usize> = (0..85).map(|_| rng.below(GLYPHS)).collect();
                let r = self.text_tokens(0, 380.0, 830.0 + k as f32 * 24.0, &toks, text, 0);
                self.log_runs.push(r);
            }
            self.log_rect = IRect::new(379, 829, 380 + 85 * 15 + 1, 830 + 5 * 24 + 1);
        }
        // scrollable table in clip 1
        let clip = IRect::new(360, 960, 1860, 1360);
        self.set_clip(1, clip);
        self.table_clip = clip;
        let ts = self.begin_space();
        self.table_space = ts;
        self.rect(0, 360.0, 960.0, 1500.0, 400.0, panel);
        for r in 0..22 {
            let y = 960.0 + r as f32 * 56.0;
            let i = self.push(Instance {
                pos: [360.0, y], size: [1500.0, 56.0],
                color: if r % 2 == 0 { rgba(0x25, 0x29, 0x33, 255) } else { panel },
                space: ts, kind: KIND_SOLID, ext: 1, ..Default::default()
            });
            self.small.push(i as u32);
            for c in 0..5 {
                self.text(ts, 380.0 + c as f32 * 290.0, y + 16.0, &[0; 8], text, 1, &mut rng);
            }
            let i = self.push(Instance {
                pos: [1700.0, y + 12.0], size: [120.0, 32.0], color: rgba(0x2f, 0x9e, 0x63, 255),
                radius: 16.0, space: ts, kind: KIND_RRECT, ext: 1, ..Default::default()
            });
            self.small.push(i as u32);
        }
        self.end_space(ts);
        // fix up: the first table bg rect is outside the space run (space 0, unclipped) - already ok.
    }

    /// Recompute the instance page unions for [a, b) (CPU cull, 16-px kernel) and refresh the core-layout mirror
    /// for the records those instances belong to.
    fn refresh_pages(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        for pg in a / 64..=(b - 1) / 64 {
            let (lo, hi) = (pg * 64, ((pg + 1) * 64).min(self.n));
            let mut u = IRect::EMPTY;
            for k in &self.ink[lo..hi] {
                if !k.is_empty() {
                    u = u.union(k);
                }
            }
            self.page_ink[pg] = u;
            let v = if u.is_empty() { [1, 1, 0, 0] } else { [u.x0, u.y0, u.x1, u.y1] };
            unsafe { *((self.pool.add(PAGES_OFF) as *mut [i32; 4]).add(pg)) = v };
        }
        if !self.inst_rec.is_empty() {
            let (r0, r1) = (self.inst_rec[a] as usize, self.inst_rec[b - 1] as usize + 1);
            self.refresh_recs(r0, r1);
        }
    }

    /// Mirror records [a, b): slot ink, group unions (gink), page union (directory) in ONE fused pass per page.
    /// NRect union is a lane-wise min (bough-core's representation; the integration has no such pass at all).
    fn refresh_recs(&mut self, a: usize, b: usize) {
        if a >= b {
            return;
        }
        let nrec = self.recs.len();
        for pg in a / 64..=(b - 1) / 64 {
            let base = unsafe { self.pool.add(PAGEPOOL_OFF + pg * PG_BYTES) };
            let ink = unsafe { base.add(PG_INK) as *mut [i16; 4] };
            let gink = unsafe { base.add(PG_GINK) as *mut [i16; 4] };
            let mut pu = [i16::MAX; 4];
            for g in 0..8 {
                let mut gu = [i16::MAX; 4];
                for k in 0..8 {
                    let r = pg * 64 + g * 8 + k;
                    if r >= nrec {
                        break;
                    }
                    let v = self.rec_ink(r);
                    unsafe { *ink.add(g * 8 + k) = v };
                    for l in 0..4 {
                        gu[l] = gu[l].min(v[l]);
                    }
                }
                unsafe { *gink.add(g) = gu };
                for l in 0..4 {
                    pu[l] = pu[l].min(gu[l]);
                }
            }
            unsafe { *(self.pool.add(DIR_INK_OFF) as *mut [i16; 4]).add(pg) = pu };
        }
    }

    // ---------------------------------------------------------------- mutations (each records damage)

    #[inline]
    fn dmg(&mut self, r: IRect) {
        self.acc.add_rect(&r);
        self.dirty_count += 1;
    }

    /// Change one instance's colour: old ink == new ink, damage once.
    #[inline]
    pub fn set_color(&mut self, i: usize, c: u32) {
        // Runs have one colour (bough-text RunHeader.color): in the heavy-text / runs scene a glyph pick recolours
        // its whole label. The default scene keeps round 1's per-glyph semantics (comparable numbers).
        let run = if self.inst_run.is_empty() || !(self.gpu_runs || self.heavy_text) { u32::MAX } else { self.inst_run[i] };
        if run != u32::MAX {
            // text colour is a property of the run (bough-text RunHeader.color): recolour the whole label
            let (first, len) = self.runs[run as usize];
            self.run_header(run as usize).color = c;
            for k in first..first + len {
                self.set_color_one(k, c);
            }
            return;
        }
        self.set_color_one(i, c);
    }

    #[inline]
    fn set_color_one(&mut self, i: usize, c: u32) {
        let r = self.ink[i];
        if self.zob.is_empty() {
            self.inst(i).color = c;
        } else {
            let zo = self.zval(i);
            self.inst(i).color = c;
            let zn = self.zval(i);
            self.zapply(r, zo ^ zn);
        }
        self.dmg(r);
    }

    /// Move/resize an instance in its space: damage old and new ink.
    pub fn set_geom(&mut self, i: usize, pos: [f32; 2], size: [f32; 2]) {
        let old = self.ink[i];
        if !self.zob.is_empty() {
            let z = self.zval(i);
            self.zapply(old, z);
        }
        let it = self.inst(i);
        it.pos = pos;
        it.size = size;
        let new = self.compute_ink(i);
        self.ink[i] = new;
        if !self.zob.is_empty() {
            let z = self.zval(i);
            self.zapply(new, z);
        }
        self.refresh_pages(i, i + 1);
        self.dmg(old);
        self.dmg(new);
    }

    /// Rewrite one space entry; refresh ink over its run; damage old+new run bounds.
    pub fn set_space(&mut self, s: u16, off: [f32; 2]) {
        let (a, b) = self.space_run[s as usize];
        let mut old = IRect::EMPTY;
        let z_on = !self.zob.is_empty();
        for i in a..b {
            old = old.union(&self.ink[i]);
        }
        self.set_space_raw(s as usize, off);
        let mut new = IRect::EMPTY;
        for i in a..b {
            let k = self.compute_ink(i);
            self.ink[i] = k;
            new = new.union(&k);
        }
        self.refresh_pages(a, b);
        // Geometry is SALTED, not hashed: every tile the move damages gets a fresh random XOR (distinct values for
        // the old and the new bounds, so tiles in both still change). Equal tile hash then means "no geometry event
        // and the same content" (a move-and-back is simply not recognised), at O(damaged tiles), never O(instances).
        if s == self.table_space {
            // scrolling inside a clip: damage is just the viewport
            let c = self.table_clip;
            if z_on {
                let r = self.fresh();
                self.zapply(c, r);
            }
            self.dmg(c);
        } else {
            if z_on {
                let (r1, r2) = (self.fresh(), self.fresh());
                self.zapply(old, r1);
                self.zapply(new, r2);
            }
            self.dmg(old);
            self.dmg(new);
        }
    }

    pub fn invalidate_all(&mut self) {
        self.acc.add_full();
        self.salt += 1;
        self.dirty_count += 1;
    }

    // ---------------------------------------------------------------- Zobrist tile hashes

    #[inline]
    fn fresh(&mut self) -> u64 {
        self.zseed = self.zseed.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.zseed;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    pub fn enable_zobrist(&mut self) {
        self.zob = vec![0u64; crate::damage::TX * crate::damage::TY];
        for i in 0..self.n {
            let z = self.zval(i);
            self.zapply(self.ink[i], z);
        }
    }

    /// 64-bit hash of everything that decides instance i's pixels (index = paint order, absolute position,
    /// size, colours, shape params, atlas rect, clip).
    #[inline]
    pub fn zval(&self, i: usize) -> u64 {
        #[inline(always)]
        fn mix(h: u64, v: u64) -> u64 {
            let h = (h ^ v).wrapping_mul(0x9E37_79B9_7F4A_7C15);
            h ^ (h >> 29)
        }
        let it = self.inst(i);
        let off = self.space(it.space as usize);
        let c = unsafe { *((self.pool.add(CLIPS_OFF) as *const [u64; 2]).add(it.ext as usize)) };
        let px = (it.pos[0] + off[0]).to_bits() as u64 | ((it.pos[1] + off[1]).to_bits() as u64) << 32;
        let sz = it.size[0].to_bits() as u64 | (it.size[1].to_bits() as u64) << 32;
        let uv = it.uv[0] as u64 | (it.uv[1] as u64) << 16 | (it.uv[2] as u64) << 32 | (it.uv[3] as u64) << 48;
        let mut h = mix(0x5bd1_e995 ^ i as u64, it.color as u64 | (it.color2 as u64) << 32);
        h = mix(h, px);
        h = mix(h, sz);
        h = mix(h, it.radius.to_bits() as u64 | (it.bw.to_bits() as u64) << 32 | (it.kind as u64) << 31);
        h = mix(h, uv);
        h = mix(h, c[0]);
        h = mix(h, c[1]);
        mix(h, 0x2545_F491_4F6C_DD1D)
    }

    #[inline]
    fn zapply(&mut self, ink: IRect, v: u64) {
        if ink.is_empty() {
            return;
        }
        let (tx0, tx1) = ((ink.x0 / 16) as usize, ((ink.x1 - 1) / 16) as usize);
        let (ty0, ty1) = ((ink.y0 / 16) as usize, ((ink.y1 - 1) / 16) as usize);
        for ty in ty0..=ty1 {
            for h in &mut self.zob[ty * crate::damage::TX + tx0..=ty * crate::damage::TX + tx1] {
                *h ^= v;
            }
        }
    }

    pub fn set_background(&mut self, c: u32) {
        let b = self.bg_inst as usize;
        self.set_color(b, c);
    }

    // ------------------------------------------------------------- scenarios used by bench + tests

    pub fn op_one_colour(&mut self, rng: &mut Rng) {
        let mut i = self.small[rng.below(self.small.len())] as usize;
        for _ in 0..8 {
            if !self.ink[i].is_empty() {
                break;
            }
            i = self.small[rng.below(self.small.len())] as usize;
        }
        let c = rng.color();
        self.set_color(i, c);
    }
    /// Heavy-stream shape: k scattered small changes.
    pub fn op_scattered(&mut self, rng: &mut Rng, k: usize) {
        for _ in 0..k {
            self.op_one_colour(rng);
        }
    }
    /// Scroll the table viewport by dy pixels (wraps).
    pub fn op_scroll(&mut self, dy: f32) {
        self.scroll_y = (self.scroll_y - dy).rem_euclid(22.0 * 56.0 - 400.0);
        let s = self.table_space;
        self.set_space(s, [0.0, -self.scroll_y]);
    }
    /// Heavy text tick, numeric part: every one of the 84 labels changes its last 3 digits. Runs: a 2-byte `ent`
    /// write per changed glyph (bough-text's digit patch). Instances: the glyph's `uv` (8 B). Damage per glyph.
    pub fn op_numeric(&mut self, rng: &mut Rng) {
        for li in 0..self.numeric.len() {
            let r = self.numeric[li];
            let (first, len) = self.runs[r];
            let base = self.run_header(r).base as usize;
            for k in len - 3..len {
                let g = 16 + rng.below(10);
                let i = first + k;
                let zo = if self.zob.is_empty() { 0 } else { self.zval(i) };
                unsafe { *(self.pool.add(GENT_OFF) as *mut u16).add(base + k) = g as u16 };
                self.inst(i).uv = self.entry_uv(g);
                if !self.zob.is_empty() {
                    let zn = self.zval(i);
                    self.zapply(self.ink[i], zo ^ zn);
                }
                let ink = self.ink[i];
                self.dmg(ink);
            }
        }
    }

    /// Heavy text tick, log part: a new line arrives. Runs: 85 `ent` writes into the oldest ring block + 5 header
    /// `base` writes (the lines are windows). Instances: all 5 x 85 glyph `uv`s rewritten. Damage: the pane.
    pub fn op_log(&mut self, rng: &mut Rng) {
        if self.log_runs.is_empty() {
            return;
        }
        self.log_head = (self.log_head + 1) % 5;
        let newest = (self.log_head + 4) % 5;
        for j in 0..85 {
            let g = rng.below(GLYPHS);
            unsafe { *(self.pool.add(GENT_OFF) as *mut u16).add(self.log_base + newest * 85 + j) = g as u16 };
        }
        for row in 0..5 {
            let r = self.log_runs[row];
            let base = (self.log_base + ((self.log_head + row) % 5) * 85) as u32;
            self.run_header(r).base = base;
            let (first, _) = self.runs[r];
            for j in 0..85 {
                let g = unsafe { *(self.pool.add(GENT_OFF) as *const u16).add(base as usize + j) } as usize;
                self.inst(first + j).uv = self.entry_uv(g);
            }
        }
        let pane = self.log_rect.intersect(&IRect::FULL);
        if !self.zob.is_empty() {
            let r = self.fresh();
            self.zapply(pane, r);
        }
        self.dmg(pane);
    }

    pub fn op_move_card(&mut self, rng: &mut Rng) {
        let s = self.cards[rng.below(self.cards.len())];
        let off = [rng.below(41) as f32 - 20.0, rng.below(41) as f32 - 20.0];
        self.set_space(s, off);
    }
    pub fn op_resize(&mut self, rng: &mut Rng) {
        let i = self.small[rng.below(self.small.len())] as usize;
        let it = *self.inst(i);
        let dw = rng.below(9) as f32 - 4.0;
        self.set_geom(i, [it.pos[0] + dw, it.pos[1]], [(it.size[0] + dw).max(4.0), it.size[1]]);
    }
}
