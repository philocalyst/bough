//! Dashboard-like scene living directly in the shared pool buffer, plus CPU-side `ink` bounds
//! (absolute device pixels, clip-intersected, +1px AA margin) used for culling and damage.

use crate::damage::Plane;
use crate::instance::*;

pub const GLYPH_W: u16 = 14;
pub const GLYPH_H: u16 = 24;
pub const ATLAS_W: usize = 1024;
pub const ATLAS_H: usize = 128;
pub const GLYPHS: usize = 96;

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
            scroll_y: 0.0,
            dirty_count: 0,
        };
        s.write_header();
        s.build();
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
        };
        unsafe { (self.pool as *mut Hdr).write(h) };
        for s in 0..MAX_SPACES {
            self.set_space_raw(s, [0.0, 0.0]);
        }
        for c in 0..MAX_CLIPS {
            self.set_clip(c, IRect::FULL);
        }
        for pg in 0..MAX_INST / 64 {
            self.mirror_page(pg);
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
        self.mirror_page(i / 64);
        i
    }

    /// Mirror page_ink[pg] into the pool for the GPU (empty pages become x0 > x1).
    #[inline]
    fn mirror_page(&self, pg: usize) {
        let r = self.page_ink[pg];
        let v = if r.is_empty() { [1, 1, 0, 0] } else { [r.x0, r.y0, r.x1, r.y1] };
        unsafe { *((self.pool.add(PAGES_OFF) as *mut [i32; 4]).add(pg)) = v };
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
    fn text(&mut self, space: u16, x: f32, y: f32, s: &[u8], color: u32, clip: u32, rng: &mut Rng) {
        let cols = ATLAS_W / GLYPH_W as usize;
        for (k, _) in s.iter().enumerate() {
            let g = (rng.next() as usize >> 20) % GLYPHS;
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
        }
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

    /// Recompute page unions for instruction range [a,b).
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
            self.mirror_page(pg);
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
