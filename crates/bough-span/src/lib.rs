//! The CPU span raster (round 8, e2e lane).
//!
//! ROUND8 section 6 calls the cheapest cost class "CPU spans": opaque boxes and glyph rows, where a core
//! writing words beats any round trip. This is that executor for `bough-x`'s columns, as a
//! [`Backend`](bough_x::store::Backend): a column is painted in one pass, each record straight into the surface
//! the compositor reads (a `wl_shm` buffer, an offscreen buffer in tests), with no intermediate accumulator and
//! no per-tile entry lists. A repair is the same pass restricted to a set of 32-pixel tiles: every primitive is
//! cut to the damaged tiles it touches ([`Spans::cells`]), so an undamaged pixel is never read or written and a
//! translucent glyph is blended exactly once.
//!
//! Text comes from a prepared glyph image (`bough-glyph`): placing a string is arithmetic over its bytes.
//!
//! What it is not: the exact twin of a GPU function. `bough-paint`'s tile kernels are that (Q15 accumulators,
//! word-exact against the generated WGSL) and stay the executor for paints that need them (shadows, gradients,
//! anything the GPU may also shade in the same frame). The gates here are the ones that matter on a CPU-only
//! path: a repair equals a full redraw equals a fresh mount, at every buffer age.
pub mod kinds;
pub mod line;

use bough_expr::U;
use bough_glyph::Image;
use bough_x::draw::{Align, Draws, Prims};
use bough_x::kinds::{FillArch, LabelArch, NumArch, OutlineArch};
use bough_x::soft::format_num;
use bough_x::store::{Backend, ColRef, Rect, Tiles};

pub const TILE_SHIFT: u32 = 5;

/// A set of 32-pixel tiles: one bit each, `stride` words per tile row. The same geometry as `bough-x`'s damage.
#[derive(Clone)]
pub struct TileSet {
    pub stride: usize,
    pub rows: usize,
    pub bits: Vec<u64>,
    /// Tile rows that may hold a bit.
    pub lo: usize,
    pub hi: usize,
}
impl TileSet {
    pub fn new(w: i32, h: i32) -> TileSet {
        let cols = ((w + 31) >> TILE_SHIFT) as usize;
        let rows = ((h + 31) >> TILE_SHIFT) as usize;
        let stride = cols.div_ceil(64).max(1);
        TileSet { stride, rows, bits: vec![0; stride * rows], lo: usize::MAX, hi: 0 }
    }
    pub fn clear(&mut self) {
        if self.lo < self.hi {
            self.bits[self.lo * self.stride..self.hi * self.stride].fill(0);
        }
        (self.lo, self.hi) = (usize::MAX, 0);
    }
    pub fn any(&self) -> bool {
        self.lo < self.hi
    }
    /// Add the tiles of `t` (same window).
    pub fn add(&mut self, t: &Tiles) {
        debug_assert!(t.stride == self.stride && t.rows == self.rows);
        if t.lo < t.hi {
            for (d, s) in self.bits[t.lo * self.stride..t.hi * self.stride].iter_mut().zip(&t.bits[t.lo * t.stride..t.hi * t.stride]) {
                *d |= *s;
            }
            self.lo = self.lo.min(t.lo);
            self.hi = self.hi.max(t.hi);
        }
    }
    pub fn add_set(&mut self, t: &TileSet) {
        if t.lo < t.hi {
            for (d, s) in self.bits[t.lo * self.stride..t.hi * self.stride].iter_mut().zip(&t.bits[t.lo * t.stride..t.hi * t.stride]) {
                *d |= *s;
            }
            self.lo = self.lo.min(t.lo);
            self.hi = self.hi.max(t.hi);
        }
    }
    pub fn fill(&mut self, w: i32, h: i32) {
        let cols = ((w + 31) >> TILE_SHIFT) as usize;
        for r in 0..self.rows.min(((h + 31) >> TILE_SHIFT) as usize) {
            for c in 0..cols {
                self.bits[r * self.stride + (c >> 6)] |= 1 << (c & 63);
            }
        }
        (self.lo, self.hi) = (0, self.rows);
    }
    pub fn count(&self) -> u32 {
        if self.lo < self.hi { self.bits[self.lo * self.stride..self.hi * self.stride].iter().map(|w| w.count_ones()).sum() } else { 0 }
    }
    /// Maximal runs of tiles in each tile row, merged down while the next row has the same run: the rectangles
    /// a compositor is told (`wl_surface.damage_buffer`), in pixels, cut to `w` x `h`.
    pub fn rects(&self, w: i32, h: i32, mut f: impl FnMut(Rect)) {
        // open rectangles: (tile x0, tile x1, tile y0), all ending at the previous row
        let mut open: [(u32, u32, u32); 8] = [(0, 0, 0); 8];
        let mut nopen = 0usize;
        let emit = |o: (u32, u32, u32), y1: u32, f: &mut dyn FnMut(Rect)| {
            f(Rect { x0: (o.0 as i32) << TILE_SHIFT, y0: (o.2 as i32) << TILE_SHIFT, x1: ((o.1 as i32) << TILE_SHIFT).min(w), y1: ((y1 as i32) << TILE_SHIFT).min(h) })
        };
        for ty in self.lo.min(self.hi)..=self.hi.min(self.rows) {
            let mut runs: [(u32, u32); 8] = [(0, 0); 8];
            let mut nruns = 0usize;
            if ty < self.hi {
                let (mut x, n) = (0u32, (self.stride * 64) as u32);
                while x < n {
                    let wd = self.bits[ty * self.stride + (x >> 6) as usize] >> (x & 63);
                    if wd == 0 {
                        x = (x | 63) + 1;
                        continue;
                    }
                    x += wd.trailing_zeros();
                    let a = x;
                    while x < n && self.bits[ty * self.stride + (x >> 6) as usize] >> (x & 63) & 1 != 0 {
                        x += 1;
                    }
                    if nruns == 8 {
                        // more runs than are tracked: widen the last one (damage may be over-reported, never under)
                        runs[7].1 = x;
                    } else {
                        runs[nruns] = (a, x);
                        nruns += 1;
                    }
                }
            }
            let mut keep = [false; 8];
            let mut k = 0;
            while k < nopen {
                let o = open[k];
                match runs[..nruns].iter().position(|r| (r.0, r.1) == (o.0, o.1)) {
                    Some(j) => {
                        keep[j] = true;
                        k += 1;
                    }
                    None => {
                        emit(o, ty as u32, &mut f);
                        nopen -= 1;
                        open[k] = open[nopen];
                    }
                }
            }
            for j in 0..nruns {
                if !keep[j] {
                    open[nopen] = (runs[j].0, runs[j].1, ty as u32);
                    nopen += 1;
                }
            }
        }
    }
}

/// What a pass did (counts of work, for the cost of a frame).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub records: u64,
    pub fills: u64,
    pub px_filled: u64,
    pub glyphs: u64,
}

/// A surface of `w` x `h` XRGB8888 pixels, `stride` pixels per row, and the tiles a pass may write.
pub struct Spans<'a> {
    px: *mut u32,
    stride: usize,
    pub w: i32,
    pub h: i32,
    /// The tiles to repaint; `None`: everything.
    mask: Option<&'a TileSet>,
    pub img: &'static Image,
    pub st: Stats,
    _p: core::marker::PhantomData<&'a mut [u32]>,
}

impl<'a> Spans<'a> {
    /// A pass over `px` (rows of `stride` pixels, `h` rows), restricted to `mask`.
    pub fn new(px: &'a mut [u32], stride: usize, w: i32, h: i32, mask: Option<&'a TileSet>, img: &'static Image) -> Spans<'a> {
        assert!(w >= 0 && h >= 0 && stride >= w as usize && px.len() >= (h as usize).saturating_sub(1) * stride + w as usize);
        Spans { px: px.as_mut_ptr(), stride, w, h, mask, img, st: Stats::default(), _p: core::marker::PhantomData }
    }
    /// The same over memory the caller vouches for.
    ///
    /// # Safety
    /// `px` is valid for writes of `h` rows of `stride` pixels (the last row `w` pixels) for `'a`, and nothing
    /// else reads or writes it meanwhile.
    pub unsafe fn raw(px: *mut u32, stride: usize, w: i32, h: i32, mask: Option<&'a TileSet>, img: &'static Image) -> Spans<'a> {
        Spans { px, stride, w, h, mask, img, st: Stats::default(), _p: core::marker::PhantomData }
    }
    /// The same with the mask's lifetime erased: for a caller that owns both and keeps the mask alive for as
    /// long as the pass (`World<Spans<'static>>` names a `'static` backend type).
    ///
    /// # Safety
    /// As [`Spans::raw`], and `mask` outlives the returned pass.
    pub unsafe fn erased(px: *mut u32, stride: usize, w: i32, h: i32, mask: Option<&TileSet>, img: &'static Image) -> Spans<'static> {
        // SAFETY: the caller keeps `mask` alive for the pass.
        let mask: Option<&'static TileSet> = unsafe { core::mem::transmute(mask) };
        Spans { px, stride, w, h, mask, img, st: Stats::default(), _p: core::marker::PhantomData }
    }

    /// The parts of `r` (cut to the surface) that lie in tiles of the mask, as rectangles: per tile row, one per
    /// maximal run of tiles.
    #[inline(always)]
    fn cells(&self, r: Rect, mut f: impl FnMut(Rect)) {
        let r = Rect { x0: r.x0.max(0), y0: r.y0.max(0), x1: r.x1.min(self.w), y1: r.y1.min(self.h) };
        if (r.x0 >= r.x1) | (r.y0 >= r.y1) {
            return;
        }
        let Some(m) = self.mask else { return f(r) };
        let (ta, tb) = ((r.x0 >> TILE_SHIFT) as u32, ((r.x1 - 1) >> TILE_SHIFT) as u32);
        let (tc, td) = ((r.y0 >> TILE_SHIFT) as usize, ((r.y1 - 1) >> TILE_SHIFT) as usize);
        for ty in tc.max(m.lo)..(td + 1).min(m.hi) {
            let (y0, y1) = (r.y0.max((ty as i32) << TILE_SHIFT), r.y1.min((ty as i32 + 1) << TILE_SHIFT));
            let row = &m.bits[ty * m.stride..(ty + 1) * m.stride];
            let mut x = ta;
            while x <= tb {
                let wd = row[(x >> 6) as usize] >> (x & 63);
                if wd == 0 {
                    x = (x | 63) + 1;
                    continue;
                }
                x += wd.trailing_zeros();
                if x > tb {
                    break;
                }
                let a = x;
                // the run: set bits from `a`, within this word, up to the rect's last tile
                let ones = (!(row[(a >> 6) as usize] >> (a & 63))).trailing_zeros();
                x = (a + ones).min(tb + 1).min((a | 63) + 1);
                f(Rect { x0: r.x0.max((a as i32) << TILE_SHIFT), y0, x1: r.x1.min((x as i32) << TILE_SHIFT), y1 });
            }
        }
    }

    /// Rows `y0..y1`, columns `x0..x1` as a mutable slice per row.
    ///
    /// SAFETY (private): the rect is inside the surface (`cells` cut it).
    #[inline(always)]
    unsafe fn row(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u32] {
        // SAFETY: the caller's rect lies inside the surface this pass owns.
        unsafe { core::slice::from_raw_parts_mut(self.px.add(y as usize * self.stride + x0 as usize), (x1 - x0) as usize) }
    }

    #[inline]
    fn fill(&mut self, r: Rect, c: u32) {
        let this = self as *mut Self;
        self.cells(r, |q| {
            // SAFETY: `cells` only reads the mask and the surface's size; the fill writes pixels. `q` is inside
            // the surface.
            let s = unsafe { &mut *this };
            s.st.fills += 1;
            s.st.px_filled += q.area() as u64;
            for y in q.y0..q.y1 {
                unsafe { s.row(y, q.x0, q.x1) }.fill(c);
            }
        });
    }

    /// One glyph's coverage at (gx, gy) blended in colour `c`, cut to `cut`.
    #[inline]
    fn blit(&mut self, gx: i32, gy: i32, gw: i32, gh: i32, cov: &[u8], cut: Rect, c: u32) {
        let this = self as *mut Self;
        let whole = Rect { x0: gx, y0: gy, x1: gx + gw, y1: gy + gh }.and(cut);
        let (cr, cg, cb) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
        let body = |q: Rect| {
            // SAFETY: as `fill`.
            let s = unsafe { &mut *this };
            for y in q.y0..q.y1 {
                let src = &cov[((y - gy) * gw + (q.x0 - gx)) as usize..((y - gy) * gw + (q.x1 - gx)) as usize];
                let dst = unsafe { s.row(y, q.x0, q.x1) };
                for (d, &a) in dst.iter_mut().zip(src) {
                    if a != 0 {
                        *d = blend(*d, cr, cg, cb, a as u32);
                    }
                }
            }
        };
        if cfg!(feature = "mutant_straddle") && self.mask.is_some() {
            let mut hit = false;
            self.cells(whole, |_| hit = true);
            if hit {
                let r = Rect { x0: whole.x0.max(0), y0: whole.y0.max(0), x1: whole.x1.min(self.w), y1: whole.y1.min(self.h) };
                if !r.empty() {
                    body(r);
                }
            }
        } else {
            self.cells(whole, body);
        }
    }

    /// The baseline of a line of text centred in a box, and the first pen column for an alignment.
    #[inline]
    pub fn text_origin(img: &Image, r: Rect, advance: i32, align: Align) -> (i32, i32) {
        let (asc, desc) = (img.px(img.ascent), img.px(img.descent));
        let base = r.y0 + (((r.y1 - r.y0) as f32 - (asc + desc)) * 0.5 + asc + 0.5).floor() as i32;
        let w = img.snap(advance);
        let x = match align {
            Align::Left => r.x0,
            Align::Right => r.x1 - w,
            Align::Center => r.x0 + ((r.x1 - r.x0) - w) / 2,
        };
        (x, base)
    }
}

/// `d` under colour (r, g, b) at coverage `a` (0..=255), per channel, rounded: `(s a + d (255 - a)) / 255`.
#[inline(always)]
fn blend(d: u32, r: u32, g: u32, b: u32, a: u32) -> u32 {
    let ia = 255 - a;
    let ch = |s: u32, d: u32| {
        let t = s * a + d * ia + 128;
        (t + (t >> 8)) >> 8
    };
    0xff00_0000 | ch(r, (d >> 16) & 0xff) << 16 | ch(g, (d >> 8) & 0xff) << 8 | ch(b, d & 0xff)
}

impl Prims for Spans<'_> {
    #[inline]
    fn solid(&mut self, r: Rect, c: u32) {
        self.fill(r, c | 0xff00_0000)
    }
    #[inline]
    fn text(&mut self, r: Rect, cut: Rect, s: &[u8], align: Align, c: u32) {
        if s.is_empty() {
            return;
        }
        // the whole line outside the mask: nothing to place
        let mut any = false;
        self.cells(r.and(cut), |_| any = true);
        if !any {
            return;
        }
        let img = self.img;
        let adv = if align == Align::Left { 0 } else { img.advance(s) };
        let (x0, base) = Self::text_origin(img, r, adv, align);
        let this = self as *mut Self;
        img.place(s, |g, pen| {
            let gl = img.glyph(g);
            if gl.w != 0 {
                // SAFETY: `place` reads the image only; the blit writes pixels.
                let sp = unsafe { &mut *this };
                sp.st.glyphs += 1;
                sp.blit(x0 + img.snap(pen) + gl.left, base - gl.top, gl.w as i32, gl.h as i32, gl.cov, cut, c);
            }
        });
    }
    fn rbox(&mut self, r: Rect, cut: Rect, radius: i32, turn: u32, c: u32) {
        // the turned box stays inside the circle through its corners
        let (cx, cy) = ((r.x0 + r.x1) as f32 * 0.5, (r.y0 + r.y1) as f32 * 0.5);
        let (hx, hy) = ((r.x1 - r.x0) as f32 * 0.5, (r.y1 - r.y0) as f32 * 0.5);
        let reach = (hx * hx + hy * hy).sqrt().ceil() as i32 + 1;
        let bb = Rect { x0: cx.floor() as i32 - reach, y0: cy.floor() as i32 - reach, x1: cx.ceil() as i32 + reach, y1: cy.ceil() as i32 + reach }.and(cut);
        let a = turn as f32 * (core::f32::consts::TAU / 65536.0);
        let (sn, cs) = a.sin_cos();
        let rad = radius as f32;
        let (cr, cg, cb) = ((c >> 16) & 0xff, (c >> 8) & 0xff, c & 0xff);
        let this = self as *mut Self;
        self.cells(bb, |q| {
            // SAFETY: as `fill`.
            let s = unsafe { &mut *this };
            for y in q.y0..q.y1 {
                let dst = unsafe { s.row(y, q.x0, q.x1) };
                let dy = y as f32 + 0.5 - cy;
                for (i, d) in dst.iter_mut().enumerate() {
                    let dx = (q.x0 + i as i32) as f32 + 0.5 - cx;
                    // the pixel centre in the box's own frame
                    let (px, py) = (dx * cs + dy * sn, dy * cs - dx * sn);
                    let (qx, qy) = (px.abs() - (hx - rad), py.abs() - (hy - rad));
                    let dist = (qx.max(0.0) * qx.max(0.0) + qy.max(0.0) * qy.max(0.0)).sqrt() + qx.max(qy).min(0.0) - rad;
                    let cov = (0.5 - dist).clamp(0.0, 1.0);
                    if cov > 0.0 {
                        *d = blend(*d, cr, cg, cb, (cov * 255.0 + 0.5) as u32);
                    }
                }
            }
        });
    }
}

impl Backend for Spans<'static> {
    fn fill<A: FillArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.st.records += 1;
            self.fill(r.shift(org).and(clip), a.color(u).0 | 0xff00_0000)
        });
    }
    fn num<A: NumArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.st.records += 1;
            let mut buf = [0u8; 24];
            let n = format_num(a.value(u), A::DECIMALS, &mut buf);
            self.text(r.shift(org), clip, &buf[..n], Align::Right, a.color(u).0)
        });
    }
    fn label<A: LabelArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.st.records += 1;
            let (b, n) = a.text().bytes();
            self.text(r.shift(org), clip, &b[..n], Align::Left, a.color(u).0)
        });
    }
    fn outline<A: OutlineArch>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.st.records += 1;
            let r = r.shift(org).shift(a.offset(u));
            let (w, c) = (A::WIDTH, a.color(u).0 | 0xff00_0000);
            for e in [Rect { y1: r.y0 + w, ..r }, Rect { y0: r.y1 - w, ..r }, Rect { x1: r.x0 + w, ..r }, Rect { x0: r.x1 - w, ..r }] {
                self.fill(e.and(clip), c);
            }
        });
    }
    fn draws<A: Draws>(&mut self, col: ColRef<'_, A>, u: &U) {
        let (org, clip) = (col.space.origin(u), col.space.clip);
        col.each(|r, a| {
            self.st.records += 1;
            a.draw(r.shift(org), clip, u, self)
        });
    }
}
