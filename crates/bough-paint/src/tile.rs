//! The CPU rasteriser of one 32 x 32 tile, as sinks. It does what the generic kernel's `shade_tile` does
//! (cores as spans, constant spans along the straight edges of a rounded box, a shadow as the product of two
//! profiles away from its corners, flat rows copied from the row above) with one difference that is the
//! point: each method here is one kind, and the paint type that calls it is known when the archetype is
//! compiled. Nothing is decoded and nothing is switched on per row. It adds the static cut: a paint that
//! knows its own upper layer is opaque hides that layer's core from its lower layers (`hide`).
//!
//! Two passes per tile, both driven by the tile's entries through their archetypes: `Flat` classifies rows
//! (a row every command agrees is the same as the row above is copied, not shaded), then `Shade` shades the
//! rows that are left into a Q15 accumulator, which `store` writes out.
use crate::paint::{EMPTY, Sink};
use bough_kernel::k;
use bough_kernel::simd;
use bough_kernel::Cmd;
use wgsl_rs::std::{Vec4, Vec4i, Vec4u};

/// One worker's tile state.
pub struct Tile<'a> {
    acc: Box<[[u16; 4]; 1024]>,
    pub ox: i32,
    pub oy: i32,
    cols: i32,
    /// Rows of the tile inside the frame.
    all: u32,
    /// Rows provably equal to the row above.
    same: u32,
    /// Rows to shade.
    need: u32,
    /// Pixels an opaque upper layer of the current record will replace (window coordinates).
    hide: Vec4i,
    /// Per entry of the tile's list: the pixels (a bit per column, a word per row) that opaque covers of
    /// *later* entries replace. An entry never shades them: the occlusion cut at pixel grain.
    masks: Vec<[u32; 32]>,
    /// The entry being shaded.
    pub cur: usize,
    pub atlas: &'a [u8],
    pub aw: usize,
    /// The corner tables (`paint::corner_taps`).
    pub tabs: &'a [u32],
    pub st: Stats,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub tiles: u64,
    pub entries: u64,
    pub rows_shaded: u64,
    pub rows_copied: u64,
    /// Shadow pixels given a coverage from the two profiles, and those at corners that went through the
    /// kernel's per-pixel integral.
    pub px_shadow: u64,
    pub px_corner: u64,
    /// Of those, the ones answered from a corner table.
    pub px_table: u64,
}
impl Stats {
    pub fn add(&mut self, o: &Stats) {
        self.tiles += o.tiles;
        self.entries += o.entries;
        self.rows_shaded += o.rows_shaded;
        self.rows_copied += o.rows_copied;
        self.px_shadow += o.px_shadow;
        self.px_corner += o.px_corner;
        self.px_table += o.px_table;
    }
}

/// Maximal runs of set bits, as [a, b) column ranges.
#[derive(Clone, Copy)]
struct Runs(u32);
impl Iterator for Runs {
    type Item = (i32, i32);
    #[inline(always)]
    fn next(&mut self) -> Option<(i32, i32)> {
        let v = self.0;
        if v == 0 {
            return None;
        }
        let a = v.trailing_zeros();
        let n = (!(v >> a)).trailing_zeros();
        self.0 = if a + n >= 32 { 0 } else { v & !(((1u32 << n) - 1) << a) };
        Some((a as i32, (a + n) as i32))
    }
}

/// Bits [a, b) of a 32-row mask.
#[inline(always)]
fn rows(a: i32, b: i32) -> u32 {
    let (a, b) = (a.clamp(0, 32) as u32, b.clamp(0, 32) as u32);
    if a >= b { 0 } else { (((1u64 << b) - 1) as u32) & !(((1u64 << a) - 1) as u32) }
}
#[inline(always)]
fn ld(p: &[u16; 4]) -> Vec4u {
    Vec4 { x: p[0] as u32, y: p[1] as u32, z: p[2] as u32, w: p[3] as u32 }
}
#[inline(always)]
fn st(v: Vec4u) -> [u16; 4] {
    [v.x as u16, v.y as u16, v.z as u16, v.w as u16]
}
#[inline(always)]
fn over(row: &mut [[u16; 4]], sh: Vec4u) {
    if sh.w == k::ONE {
        row.fill(st(sh));
    } else if sh.w != 0 || sh.x != 0 || sh.y != 0 || sh.z != 0 {
        simd::blend_const(row, st(sh));
    }
}

impl<'a> Tile<'a> {
    pub fn new(atlas: &'a [u8], aw: u32, tabs: &'a [u32]) -> Tile<'a> {
        Tile { acc: Box::new([[0; 4]; 1024]), ox: 0, oy: 0, cols: 0, all: 0, same: 0, need: 0, hide: EMPTY, masks: Vec::new(), cur: 0, atlas, aw: aw as usize, tabs, st: Stats::default() }
    }
    /// Start a tile at (ox, oy) with `cols` x `nrows` pixels inside the frame.
    pub fn begin(&mut self, ox: i32, oy: i32, cols: usize, nrows: usize, flat: bool) {
        (self.ox, self.oy, self.cols) = (ox, oy, cols as i32);
        self.all = rows(0, nrows as i32);
        // row 0 has no row above it in the tile
        self.same = if flat { self.all & !1 } else { 0 };
        self.hide = EMPTY;
    }
    /// The occlusion masks for this tile's list (paint-order keys, back to front) from the cover lane: walk
    /// the list from the top, handing each entry the union of the covers above it.
    pub fn cover_masks(&mut self, list: &[u32], cover: &[[i16; 4]], on: bool) {
        self.masks.clear();
        self.masks.resize(list.len(), [0; 32]);
        if !on {
            return;
        }
        let mut run = [0u32; 32];
        for j in (0..list.len()).rev() {
            self.masks[j] = run;
            let c = cover[list[j] as usize];
            if c[0] < c[2] {
                let x1 = if cfg!(feature = "mutant_mask") { c[2] as i32 + 1 } else { c[2] as i32 };
                let bits = rows(c[0] as i32 - self.ox, x1 - self.ox);
                let mut m = rows(c[1] as i32 - self.oy, c[3] as i32 - self.oy);
                while m != 0 {
                    run[m.trailing_zeros() as usize] |= bits;
                    m &= m - 1;
                }
            }
        }
    }
    /// After the flat pass: decide the rows to shade and clear them.
    pub fn prepare(&mut self) {
        self.need = self.all & !self.same;
        let mut m = self.need;
        while m != 0 {
            let y = m.trailing_zeros() as usize;
            m &= m - 1;
            self.acc[y * 32..y * 32 + 32].fill([0; 4]);
        }
    }
    /// Write the tile: shaded rows from the accumulator, flat rows from the row above.
    ///
    /// # Safety
    /// `dst` points at the tile's first pixel in a surface of `stride` words per row; the tile's rows and
    /// columns inside the frame (`begin`) are valid for writes and nothing else reads or writes them
    /// meanwhile.
    pub unsafe fn store(&mut self, dst: *mut u32, stride: usize) {
        let n = self.cols as usize;
        let mut m = self.all;
        while m != 0 {
            let y = m.trailing_zeros() as usize;
            m &= m - 1;
            // SAFETY: row `y` of this tile, `n` columns: inside the tile (the caller's contract).
            let row = unsafe { std::slice::from_raw_parts_mut(dst.add(y * stride), n) };
            if self.need >> y & 1 != 0 {
                simd::store_row(&self.acc[y * 32..y * 32 + n], row);
            } else {
                // SAFETY: a flat row is never row 0, so row `y - 1` is this tile's own, already written.
                let src = unsafe { std::slice::from_raw_parts(dst.add((y - 1) * stride), n) };
                match (<&mut [u32; 32]>::try_from(&mut *row), <&[u32; 32]>::try_from(src)) {
                    (Ok(d), Ok(s)) => *d = *s,
                    _ => row.copy_from_slice(src),
                }
            }
        }
        self.st.tiles += 1;
        self.st.rows_shaded += self.need.count_ones() as u64;
        self.st.rows_copied += (self.all & self.same).count_ones() as u64;
    }
    /// The command's columns in the tile and its rows among those to shade.
    #[inline(always)]
    fn span(&self, c: &Cmd) -> (i32, i32, u32) {
        ((c.win.x - self.ox).max(0), (c.win.z - self.ox).min(self.cols), rows(c.win.y - self.oy, c.win.w - self.oy) & self.need)
    }
    /// The columns of [x0, x1) on tile row `y` that this entry must shade: not under a later entry's opaque
    /// cover, not under its own record's opaque upper layer. As maximal runs.
    #[inline(always)]
    fn runs(&self, y: i32, x0: i32, x1: i32) -> Runs {
        let mut vis = rows(x0, x1) & !self.masks[self.cur][y as usize];
        let (h, gy) = (self.hide, self.oy + y);
        if gy >= h.y && gy < h.w {
            vis &= !rows(h.x - self.ox, h.z - self.ox);
        }
        Runs(vis)
    }
    #[inline(always)]
    fn core_x(&self, c: &Cmd) -> (i32, i32) {
        let cz = if cfg!(feature = "mutant_core") { c.core.z + 1 } else { c.core.z };
        (c.core.x - self.ox, cz - self.ox)
    }

    /// A rounded box or a border: per row, corner pixels through the kernel's distance, the span between
    /// them at one coverage.
    #[inline(always)]
    fn boxy<const BORDER: bool>(&mut self, c: Cmd) {
        let (x0, x1, mut m) = self.span(&c);
        if x0 >= x1 {
            return;
        }
        let cv = if BORDER { c.pm2 } else { c.pm };
        let (pm, pm2) = (st(c.pm), st(c.pm2));
        let (kx0, kx1) = self.core_x(&c);
        let (ox, oy) = (self.ox, self.oy);
        let (mut cov, mut cov2) = ([0u16; 32], [0u16; 32]);
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            let gy = oy + y;
            let cy = gy * 256 + 128;
            for (s0, s1) in self.runs(y, x0, x1) {
                if s0 >= s1 {
                    continue;
                }
                let row = &mut self.acc[(y * 32) as usize..(y * 32 + 32) as usize];
                // the row's constant span: the core on a core row; on a top or bottom edge row the pixels
                // between the corners, whose distance is `qy - r` whatever x is
                let (a, b, mid) = if gy >= c.core.y && gy < c.core.w && kx0 < kx1 {
                    (kx0.clamp(s0, s1), kx1.clamp(s0, s1), cv)
                } else {
                    let r = c.rad.x;
                    let qy = (cy - c.b.w).max(c.b.y - cy) + r;
                    let e = (-qy).max(0);
                    let (lo, hi) = (c.b.x + r + e, c.b.z - r - e);
                    let sa = (((lo + 127) >> 8) - ox).clamp(s0, s1);
                    let sb = (((hi - 128) >> 8) + 1 - ox).clamp(sa, s1);
                    let d = if qy > 0 { (((qy as u32 >> c.par.y).min(16383) << c.par.y) as i32) - r } else { qy - r };
                    let outer = k::cov(d);
                    let sh = if BORDER {
                        let inner = k::cov(d + c.rad.y).min(outer);
                        k::scale(c.pm, outer - inner) + k::scale(c.pm2, inner)
                    } else {
                        k::scale(c.pm, outer)
                    };
                    (sa, sb, sh)
                };
                for (ea, eb) in [(s0, a), (b, s1)] {
                    if ea >= eb {
                        continue;
                    }
                    let n = (eb - ea) as usize;
                    let cxa = (ox + ea) * 256 + 128;
                    let r = &mut row[ea as usize..eb as usize];
                    if BORDER {
                        for i in 0..n {
                            let d = k::sd(cxa + i as i32 * 256, cy, c.b, c.rad.x, c.par.y);
                            let outer = k::cov(d);
                            let inner = k::cov(d + c.rad.y).min(outer);
                            (cov[i], cov2[i]) = ((outer - inner) as u16, inner as u16);
                        }
                        simd::blend_cov2(r, pm, &cov[..n], pm2, &cov2[..n]);
                    } else {
                        for (i, o) in cov[..n].iter_mut().enumerate() {
                            *o = k::cov(k::sd(cxa + i as i32 * 256, cy, c.b, c.rad.x, c.par.y)) as u16;
                        }
                        simd::blend_cov(r, pm, &cov[..n]);
                    }
                }
                if a < b {
                    over(&mut row[a as usize..b as usize], mid);
                }
            }
        }
    }
}

/// Pass 1: which rows equal the row above.
pub struct Flat<'t, 'a>(pub &'t mut Tile<'a>);
impl Flat<'_, '_> {
    #[inline(always)]
    fn note(&mut self, c: &Cmd, flat_core: bool, all_flat: bool) {
        let t = &mut *self.0;
        if c.win.x >= t.ox + 32 || c.win.z <= t.ox {
            return;
        }
        let win = rows(c.win.y - t.oy, c.win.w - t.oy);
        let flat = if all_flat {
            win
        } else if flat_core {
            if cfg!(feature = "mutant_flat") { win } else { rows(c.core.y - t.oy, c.core.w - t.oy) & win }
        } else {
            0
        };
        t.same &= (flat & flat << 1) | (!win & !(win << 1));
    }
}
impl Sink for Flat<'_, '_> {
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        self.note(&c, false, true)
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.note(&c, true, false)
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.note(&c, true, false)
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        self.note(&c, true, false)
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        // a gradient with no vertical component is one colour per column on its core rows
        self.note(&c, c.rad.w == 0, false)
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        self.note(&c, false, false)
    }
}

/// Pass 2: shade the rows that need it.
pub struct Shade<'t, 'a>(pub &'t mut Tile<'a>);
impl Sink for Shade<'_, '_> {
    #[inline(always)]
    fn hide(&mut self, core: Vec4i) -> Vec4i {
        let prev = self.0.hide;
        if core.x < core.z {
            self.0.hide = if cfg!(feature = "mutant_hide") { Vec4 { x: core.x - 1, y: core.y, z: core.z, w: core.w } } else { core };
        }
        prev
    }
    #[inline(always)]
    fn show(&mut self, prev: Vec4i) {
        self.0.hide = prev;
    }
    #[inline(always)]
    fn solid(&mut self, c: Cmd) {
        let t = &mut *self.0;
        let (x0, x1, mut m) = t.span(&c);
        if x0 >= x1 {
            return;
        }
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            for (a, b) in t.runs(y, x0, x1) {
                if a < b {
                    over(&mut t.acc[(y * 32 + a) as usize..(y * 32 + b) as usize], c.pm);
                }
            }
        }
    }
    #[inline(always)]
    fn rrect(&mut self, c: Cmd) {
        self.0.boxy::<false>(c)
    }
    #[inline(always)]
    fn border(&mut self, c: Cmd) {
        self.0.boxy::<true>(c)
    }
    #[inline(always)]
    fn shadow(&mut self, c: Cmd) {
        let t = &mut *self.0;
        let (x0, x1, mut m) = t.span(&c);
        if x0 >= x1 {
            return;
        }
        let pm = st(c.pm);
        let (kx0, kx1) = t.core_x(&c);
        let (ox, oy) = (t.ox, t.oy);
        // the column profile over this tile's columns, made at the first row that needs it
        let (mut sx, mut cov) = ([0u16; 32], [0u16; 32]);
        let (mut zone_x, mut sx_ready) = (0u32, false);
        let (hx, hy, lim) = ((c.b.z - c.b.x) >> 1, (c.b.w - c.b.y) >> 1, c.rad.y + (c.rad.y >> 1));
        let (tabs, ok) = (t.tabs, crate::paint::tab_ok(c, t.tabs));
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            let gy = oy + y;
            let cy = gy * 256 + 128;
            let core_row = gy >= c.core.y && gy < c.core.w && kx0 < kx1;
            for (s0, s1) in t.runs(y, x0, x1) {
                if s0 >= s1 {
                    continue;
                }
                let (a, b) = if core_row { (kx0.clamp(s0, s1), kx1.clamp(s0, s1)) } else { (s1, s1) };
                if !sx_ready && (s0 < a || b < s1) {
                    for (i, o) in sx[..(x1 - x0) as usize].iter_mut().enumerate() {
                        let px = (ox + x0 + i as i32) * 256 + 128 - (c.b.x + hx);
                        *o = k::span3(px, hx, c.rad.y, c.rad.z) as u16;
                        if c.rad.x != 0 && px.abs() > hx - c.rad.x - lim {
                            zone_x |= 1 << i;
                        }
                    }
                    sx_ready = true;
                }
                let py = cy - (c.b.y + hy);
                let (sy, zone_y) = (k::span3(py, hy, c.rad.y, c.rad.z) as u16, c.rad.x != 0 && py.abs() > hy - c.rad.x - lim);
                let row = &mut t.acc[(y * 32) as usize..(y * 32 + 32) as usize];
                for (ea, eb) in [(s0, a), (b, s1)] {
                    if ea >= eb {
                        continue;
                    }
                    let (n, i0) = ((eb - ea) as usize, (ea - x0) as usize);
                    simd::scale_row(&sx[i0..i0 + n], sy, &mut cov[..n]);
                    t.st.px_shadow += n as u64;
                    if zone_y {
                        // the corner corrections: the kernel, pixel by pixel, where a corner is in reach
                        let mut zm = (zone_x >> i0) & (u32::MAX >> (32 - n));
                        t.st.px_corner += zm.count_ones() as u64;
                        while zm != 0 {
                            let i = zm.trailing_zeros() as usize;
                            zm &= zm - 1;
                            let (x, v) = (ox + ea + i as i32, crate::paint::corner_taps(c, ox + ea + i as i32, gy, ok, tabs));
                            t.st.px_table += (ok && !v.contains(&crate::ops::ops::T_MISS)) as u64;
                            cov[i] = crate::ops::ops::x_shadowm(c, x, gy, v[0], v[1], v[2], v[3]) as u16;
                        }
                    }
                    simd::blend_cov(&mut row[ea as usize..eb as usize], pm, &cov[..n]);
                }
                if a < b {
                    over(&mut row[a as usize..b as usize], c.pm);
                }
            }
        }
    }
    #[inline(always)]
    fn grad(&mut self, c: Cmd) {
        let t = &mut *self.0;
        let (x0, x1, mut m) = t.span(&c);
        if x0 >= x1 {
            return;
        }
        let (kx0, kx1) = t.core_x(&c);
        let (ox, oy) = (t.ox, t.oy);
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            let gy = oy + y;
            let cy = gy * 256 + 128;
            let core_row = gy >= c.core.y && gy < c.core.w && kx0 < kx1;
            for (s0, s1) in t.runs(y, x0, x1) {
                if s0 >= s1 {
                    continue;
                }
                let (a, b) = if core_row { (kx0.clamp(s0, s1), kx1.clamp(s0, s1)) } else { (s1, s1) };
                let row = &mut t.acc[(y * 32) as usize..(y * 32 + 32) as usize];
                for (ea, eb) in [(s0, a), (b, s1)] {
                    for xx in ea..eb {
                        let p = &mut row[xx as usize];
                        *p = st(k::blend(ld(p), k::sh_grad(c, (ox + xx) * 256 + 128, cy)));
                    }
                }
                if a < b {
                    if c.rad.z == 0 {
                        // a vertical gradient is one colour per row
                        over(&mut row[a as usize..b as usize], k::grad_color(c, (ox + a) * 256 + 128, cy));
                    } else {
                        for xx in a..b {
                            let p = &mut row[xx as usize];
                            *p = st(k::blend(ld(p), k::grad_color(c, (ox + xx) * 256 + 128, cy)));
                        }
                    }
                }
            }
        }
    }
    #[inline(always)]
    fn glyph(&mut self, c: Cmd) {
        let t = &mut *self.0;
        let (x0, x1, mut m) = t.span(&c);
        if x0 >= x1 {
            return;
        }
        let pm = st(c.pm);
        // its column range and first texel are the same on every row; each row is a coverage row of its
        // texels under one vector blend
        let (gw, gh) = ((c.b.z - c.b.x) >> 8, (c.b.w - c.b.y) >> 8);
        let tx0 = ((t.ox + x0) * 256 + 128 - c.b.x) >> 8;
        let (xa, xb) = (x0 + (-tx0).max(0), x1.min(x0 + gw - tx0));
        if xa >= xb {
            return;
        }
        let (n, tcol) = ((xb - xa) as usize, (tx0 + (xa - x0)) as usize);
        let mut cov = [0u16; 32];
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            let ty = ((t.oy + y) * 256 + 128 - c.b.y) >> 8;
            if ty < 0 || ty >= gh {
                continue;
            }
            let first = c.par.z as usize + ty as usize * t.aw + tcol;
            match t.atlas.get(first..first + n) {
                Some(tex) => {
                    if !simd::q15_row(tex, &mut cov[..n]) {
                        continue;
                    }
                }
                // bounds by construction: clamp each index, as the kernel's fetch does
                None => {
                    if t.atlas.is_empty() {
                        continue;
                    }
                    for (i, o) in cov[..n].iter_mut().enumerate() {
                        *o = if t.atlas.is_empty() { 0 } else { k::q15(t.atlas[(first + i).min(t.atlas.len() - 1)] as u32) as u16 };
                    }
                }
            }
            for (a, b) in t.runs(y, xa, xb) {
                simd::blend_cov(&mut t.acc[(y * 32 + a) as usize..(y * 32 + b) as usize], pm, &cov[(a - xa) as usize..(b - xa) as usize]);
            }
        }
    }
}
