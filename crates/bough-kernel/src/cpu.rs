//! The CPU executors. `naive` is the kernel's definition run pixel by pixel over every command (the
//! specification, and the gates' oracle on any host). `raster` is the fast one: per-tile programs with the
//! occlusion cut, spans and cores, flat-row replication, bands of tile rows across cores. They, and the GPU,
//! call the same `kernel.rs` functions; their equality is the crate's test.
use crate::k::{self, Cmd};
use crate::stage::{F_TO, Scene, WORDS};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Condvar, Mutex};
use wgsl_rs::std::{Vec4, Vec4u};

/// The textures commands read: the glyph atlas (one coverage byte per texel, `w` texels per row) and the
/// image atlas (premultiplied RGBA8, four bytes per texel, `iw` texels per row; may be empty).
#[derive(Clone, Copy)]
pub struct Atlas<'a> {
    pub px: &'a [u8],
    pub w: u32,
    pub img: &'a [u8],
    pub iw: u32,
}

impl<'a> Atlas<'a> {
    /// Glyphs only.
    pub fn new(px: &'a [u8], w: u32) -> Atlas<'a> {
        Atlas { px, w, img: &[], iw: 1 }
    }
    pub fn with_images(self, img: &'a [u8], iw: u32) -> Atlas<'a> {
        Atlas { img, iw, ..self }
    }
    #[inline(always)]
    fn cov(&self, t: u32) -> u32 {
        if t == k::NO_TEXEL || self.px.is_empty() { 0 } else { self.px[(t as usize).min(self.px.len() - 1)] as u32 }
    }
    #[inline(always)]
    fn image(&self, t: u32) -> u32 {
        if t == k::NO_TEXEL || self.img.len() < 4 {
            return 0;
        }
        let i = (t as usize).min(self.img.len() / 4 - 1) * 4;
        u32::from_le_bytes([self.img[i], self.img[i + 1], self.img[i + 2], self.img[i + 3]])
    }
    /// The texels `k::shade` needs at pixel (x, y): the CPU's twin of the kernel's `fetch` (the same
    /// `k::taps`, the same clamps).
    #[inline(always)]
    pub fn fetch(&self, c: &Cmd, x: i32, y: i32) -> Vec4u {
        let kind = c.par.x;
        if kind == k::K_GLYPH {
            let t = k::taps(*c, x, y, self.w, self.iw);
            return Vec4 { x: self.cov(t.x), y: self.cov(t.y), z: self.cov(t.z), w: self.cov(t.w) };
        }
        if kind == k::K_IMAGE || kind == k::K_RAMP {
            let t = k::taps(*c, x, y, self.w, self.iw);
            return Vec4 { x: self.image(t.x), y: self.image(t.y), z: self.image(t.z), w: self.image(t.w) };
        }
        k::splat(0)
    }
}

/// A frame's tile programs, as the words the GPU kernel reads: header (`PROG_HDR` words: width, height,
/// output stride, atlas width, now, tile count), the tile table (x, y, list offset, list length per tile, in
/// row-major order) and the lists (command slots in paint order).
#[derive(Default, Clone)]
pub struct Prog {
    pub words: Vec<u32>,
}

const HDR: usize = k::PROG_HDR as usize;

impl Prog {
    pub fn fw(&self) -> u32 {
        self.words[0]
    }
    pub fn fh(&self) -> u32 {
        self.words[1]
    }
    pub fn now(&self) -> u32 {
        self.words[4]
    }
    pub fn set_now(&mut self, now: u32) {
        self.words[4] = now;
    }
    pub fn tiles(&self) -> usize {
        self.words[5] as usize
    }
    /// Tile `i`: origin and its command list.
    #[inline]
    pub fn tile(&self, i: usize) -> (i32, i32, &[u32]) {
        let t = &self.words[HDR + 4 * i..HDR + 4 * i + 4];
        (t[0] as i32, t[1] as i32, &self.words[t[2] as usize..(t[2] + t[3]) as usize])
    }
    /// Commands summed over the tiles' lists.
    pub fn entries(&self) -> usize {
        (0..self.tiles()).map(|i| self.words[HDR + 4 * i + 3] as usize).sum()
    }
}

/// Which tiles a frame repairs.
pub enum Damage<'a> {
    All,
    /// Tile coordinates (tx, ty), any order, no duplicates.
    Tiles(&'a [(u16, u16)]),
}

/// Reusable binning state (no allocation once warm).
#[derive(Default)]
pub struct Binner {
    slot: Vec<u32>,
    count: Vec<u32>,
    floor: Vec<u32>,
    /// The last program's estimated cost on one core, ns (`KIND_NS` summed over tile entries).
    pub est_cpu_ns: u64,
}

/// One core's cost of a command on a tile it touches, ns, by kind (kbench micro, M3 Pro, with flat-row
/// replication; a command rarely covers its whole tile, so these lean high for the per-pixel kinds, which
/// biases an effect-heavy frame towards the GPU). Index: kind, then for shadows and gradients whether the
/// cheap form applies (square corners; vertical).
pub const KIND_NS: [[u32; 2]; 9] = [[10, 10], [100, 100], [100, 100], [190, 190], [1900, 900], [3000, 300], [2500, 2500], [3000, 3000], [3000, 3000]];

/// How a frame is executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exec {
    /// The calling thread.
    Cpu1,
    /// The calling thread and this many pool workers.
    CpuN(usize),
    Gpu,
}

/// Measured constants of the choice (ns). `gpu_fixed`: a GPU frame's round trip whatever it draws, when the
/// GPU ran within the last few frames (`warm`) and when it has clocked down; `wake`: getting parked workers
/// going; the GPU's kernel time per tile and per tile entry.
pub struct Costs {
    pub gpu_fixed_warm: u64,
    pub gpu_fixed_cold: u64,
    pub gpu_tile: u64,
    pub gpu_entry: u64,
    pub wake: u64,
}

/// M3 Pro, 2026-11-12 (journal E5): 150 us warm and 650 us at a display's cadence; 180 ns per tile and 55 ns
/// per entry; 30 us to wake workers.
pub const COSTS: Costs = Costs { gpu_fixed_warm: 150_000, gpu_fixed_cold: 650_000, gpu_tile: 180, gpu_entry: 55, wake: 30_000 };

/// The gradient: the cheapest of one core, `workers` more cores, and the GPU, for a program of `tiles` tiles
/// and `entries` entries whose one-core estimate is `cpu_ns`. This is the whole scheduler.
pub fn choose(cpu_ns: u64, tiles: u64, entries: u64, workers: usize, gpu: bool, gpu_warm: bool, c: &Costs) -> Exec {
    let n = workers as u64 + 1;
    // bands are tile rows: a small repair has few of them, and the pool never reaches a perfect split
    let many = c.wake + cpu_ns * 5 / (4 * n);
    let g = if gpu { (if gpu_warm { c.gpu_fixed_warm } else { c.gpu_fixed_cold }) + c.gpu_tile * tiles + c.gpu_entry * entries } else { u64::MAX };
    if cpu_ns <= many.min(g) || workers == 0 && cpu_ns <= g {
        Exec::Cpu1
    } else if many <= g && workers > 0 {
        Exec::CpuN(workers)
    } else {
        Exec::Gpu
    }
}

impl Binner {
    /// Build the tile programs of the damaged tiles of a `fw` x `fh` frame. `cut`: apply the occlusion cut
    /// (identity 2). The input here is the whole command list; in the runtime it is the page cull's output
    /// for the damaged rows.
    pub fn bin(&mut self, scene: &Scene, fw: u32, fh: u32, stride: u32, atlas: &Atlas, now: u32, damage: &Damage, cut: bool, prog: &mut Prog) {
        let (aw, alim) = (atlas.w, (atlas.px.len() as u32).saturating_sub(1));
        let (tx, ty) = (fw.div_ceil(32) as usize, fh.div_ceil(32) as usize);
        self.slot.clear();
        // the damage's bounding box in tiles: a command is tested against it, not against the window (a
        // background would otherwise walk every tile of the window for a two-tile repair)
        let (mut bx0, mut by0, mut bx1, mut by1) = (0, 0, tx, ty);
        let nt = match damage {
            Damage::All => {
                self.slot.extend(0..(tx * ty) as u32);
                tx * ty
            }
            Damage::Tiles(t) => {
                let mut n = 0u32;
                (bx0, by0, bx1, by1) = (tx, ty, 0, 0);
                for &(x, y) in t.iter() {
                    (bx0, by0, bx1, by1) = (bx0.min(x as usize), by0.min(y as usize), bx1.max(x as usize + 1), by1.max(y as usize + 1));
                }
                if t.is_empty() {
                    (bx0, by0, bx1, by1) = (0, 0, 0, 0);
                }
                // the slot map covers the box only; row-major slots, so a band of tile rows is a contiguous
                // range of the table
                self.slot.resize((bx1 - bx0) * (by1 - by0), u32::MAX);
                for &(x, y) in t.iter() {
                    self.slot[(y as usize - by0) * (bx1 - bx0) + (x as usize - bx0)] = 0;
                }
                for s in self.slot.iter_mut() {
                    if *s == 0 {
                        *s = n;
                        n += 1;
                    }
                }
                n as usize
            }
        };
        let bw = bx1 - bx0;
        self.count.clear();
        self.count.resize(nt, 0);
        self.floor.clear();
        self.floor.resize(nt, 0);
        self.est_cpu_ns = 0;
        let n = scene.len() as u32;
        // a command's window and core are read straight from its words (two words each): binning never
        // decodes a command
        let rect = |w: usize| {
            let (a, b) = (scene.words[w], scene.words[w + 1]);
            [(a as i16) as i32, ((a >> 16) as i16) as i32, (b as i16) as i32, ((b >> 16) as i16) as i32]
        };
        let span = |win: [i32; 4]| {
            let x0 = ((win[0].max(0) as usize) >> 5).max(bx0);
            let y0 = ((win[1].max(0) as usize) >> 5).max(by0);
            let x1 = ((win[2].max(0) as usize + 31) >> 5).min(bx1);
            let y1 = ((win[3].max(0) as usize + 31) >> 5).min(by1);
            (x0, y0, x1, y1)
        };
        for i in 0..n {
            let w = i as usize * WORDS;
            let h = scene.words[w];
            if h & F_TO != 0 {
                continue;
            }
            let (x0, y0, x1, y1) = span(rect(w + 4));
            if x0 >= x1 || y0 >= y1 {
                continue;
            }
            let kind = h & 255;
            let (a1, a2) = (scene.words[w + 17] >> 16, scene.words[w + 19] >> 16);
            // `k::opaque` of the resolved command, from its words (a tinted command is resolved; a transition
            // never cuts: its opacity is a function of time)
            let opaque = cut
                && h & k::F_ANIM == 0
                && if h & k::F_T_ANY != 0 {
                    k::opaque(scene.load(i))
                } else {
                    match kind {
                        k::K_SOLID | k::K_RRECT | k::K_SHADOW => a1 == k::ONE || (cfg!(feature = "mutant_cut") && a1 >= k::ONE / 2),
                        k::K_BORDER => a2 == k::ONE,
                        k::K_GRAD => a1 == k::ONE && a2 == k::ONE,
                        _ => false,
                    }
                };
            let core = rect(w + 6);
            let cheap = kind == k::K_SHADOW && scene.words[w + 12] == 0 || kind == k::K_GRAD && scene.words[w + 14] == 0;
            let ns = if scene.words[w + 28] != 0 { 3000 } else { KIND_NS[(kind as usize).min(8)][cheap as usize] as u64 };
            for y in y0..y1 {
                for x in x0..x1 {
                    let s = self.slot[(y - by0) * bw + (x - bx0)];
                    if s == u32::MAX {
                        continue;
                    }
                    let (ox, oy) = (x as i32 * 32, y as i32 * 32);
                    if opaque && core[0] <= ox && core[1] <= oy && core[2] >= ox + 32 && core[3] >= oy + 32 {
                        self.floor[s as usize] = i;
                        self.count[s as usize] = 1;
                    } else {
                        self.count[s as usize] += 1;
                    }
                    // (entries a later cut removes stay in the estimate: it leans high on layered frames)
                    self.est_cpu_ns += ns;
                }
            }
        }
        let w = &mut prog.words;
        w.clear();
        w.extend_from_slice(&[fw, fh, stride, aw, now, nt as u32, alim, atlas.iw, (atlas.img.len() as u32 / 4).saturating_sub(1), 0, 0, 0]);
        w.resize(HDR + 4 * nt, 0);
        let mut off = (HDR + 4 * nt) as u32;
        for (i, &s) in self.slot.iter().enumerate() {
            if s == u32::MAX {
                continue;
            }
            let t = HDR + 4 * s as usize;
            w[t] = (bx0 + i % bw) as u32 * 32;
            w[t + 1] = (by0 + i / bw) as u32 * 32;
            w[t + 2] = off;
            off += self.count[s as usize];
        }
        w.resize(off as usize, 0);
        // the table rides at the program's tail (header words 9 and 10: its offset and last entry); an empty
        // table is one zero entry, so the kernel's clamped read always lands inside the buffer
        w[9] = off;
        w[10] = (scene.table.len() as u32 / 2).saturating_sub(1);
        w.extend_from_slice(&scene.table[..scene.table.len() & !1]);
        if scene.table.len() < 2 {
            w.extend_from_slice(&[0, 0]);
        }
        for i in 0..n {
            let wi = i as usize * WORDS;
            if scene.words[wi] & F_TO != 0 {
                continue;
            }
            let (x0, y0, x1, y1) = span(rect(wi + 4));
            for y in y0..y1 {
                for x in x0..x1 {
                    let s = self.slot[(y - by0) * bw + (x - bx0)];
                    if s == u32::MAX || i < self.floor[s as usize] {
                        continue;
                    }
                    let t = HDR + 4 * s as usize;
                    let at = (w[t + 2] + w[t + 3]) as usize;
                    w[at] = i;
                    w[t + 3] += 1;
                }
            }
        }
    }
}

/// What the fast executor did (for the cost model and the experiments).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub tiles: u64,
    /// Commands summed over tiles.
    pub entries: u64,
    pub rows_shaded: u64,
    /// Pixel rows copied from the row above (identity 4).
    pub rows_copied: u64,
    /// Tiles taken from the tile cache (identity 5).
    pub cache_hits: u64,
}

impl Stats {
    fn add(&mut self, o: &Stats) {
        self.tiles += o.tiles;
        self.entries += o.entries;
        self.rows_shaded += o.rows_shaded;
        self.rows_copied += o.rows_copied;
        self.cache_hits += o.cache_hits;
    }
}

/// One worker's reusable state.
pub struct Scratch {
    cmds: Vec<Cmd>,
    acc: Box<[[u16; 4]; 1024]>,
    pub st: Stats,
}

impl Default for Scratch {
    fn default() -> Self {
        Scratch { cmds: Vec::new(), acc: Box::new([[0; 4]; 1024]), st: Stats::default() }
    }
}

/// Executor switches (each is an identity that can be turned off to measure it).
#[derive(Clone, Copy)]
pub struct Opt {
    /// Copy flat rows from the row above.
    pub flat: bool,
    /// Fold the tile's whole list at every pixel through `k::shade`, with no core, span or flat row (the
    /// specification restricted to a tile program; `reference` uses it).
    pub spec: bool,
    /// Pixels outside this rectangle (x0, y0, x1, y1) are neither shaded nor stored: the damage's bounds
    /// inside its tiles (a changed glyph is a few hundred pixels of the two tiles it touches).
    pub clip: [i32; 4],
}

/// No clip.
pub const EVERYWHERE: [i32; 4] = [i32::MIN / 2, i32::MIN / 2, i32::MAX / 2, i32::MAX / 2];

impl Default for Opt {
    fn default() -> Self {
        Opt { flat: true, spec: false, clip: EVERYWHERE }
    }
}

impl Opt {
    pub const fn flat(flat: bool) -> Opt {
        Opt { flat, spec: false, clip: EVERYWHERE }
    }
    pub const fn clipped(clip: [i32; 4]) -> Opt {
        Opt { flat: true, spec: false, clip }
    }
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
use crate::simd::{self, store_row};
/// The vector loops (re-exported for their own test).
pub use crate::simd as vec;
/// Bits [a, b) of a 32-row mask.
#[inline(always)]
fn rows(a: i32, b: i32) -> u32 {
    let (a, b) = (a.clamp(0, 32) as u32, b.clamp(0, 32) as u32);
    if a >= b { 0 } else { (((1u64 << b) - 1) as u32) & !(((1u64 << a) - 1) as u32) }
}

/// Shade one tile whose commands are `sc.cmds` (paint order) into `dst` (`rows` rows of `stride` words, the
/// tile's first pixel at `dst[0]`, `cols` columns).
fn shade_tile(sc: &mut Scratch, ox: i32, oy: i32, atlas: &Atlas, now: u32, dst: &mut [u32], stride: usize, cols: usize, nrows: usize, opt: Opt) {
    if opt.spec {
        for y in 0..nrows {
            for x in 0..cols {
                let (gx, gy) = (ox + x as i32, oy + y as i32);
                let mut acc = k::splat(0);
                for c in sc.cmds.iter() {
                    acc = k::blend(acc, k::shade(*c, gx, gy, atlas.fetch(c, gx, gy), now));
                }
                dst[y * stride + x] = k::store8(acc);
            }
        }
        return;
    }
    // the damage's bounds inside this tile: rows and columns outside them are left as they are on the surface
    let all = rows((opt.clip[1] - oy).max(0), (opt.clip[3] - oy).min(nrows as i32));
    let (kx0, kx1) = ((opt.clip[0] - ox).max(0), (opt.clip[2] - ox).min(cols as i32));
    if all == 0 || kx0 >= kx1 {
        return;
    }
    // classify rows first (bitmasks), then shade only the set bits. (Row 0 has no row above it in the tile;
    // any other row's predecessor is on the surface, current, whether or not this frame repairs it.)
    let mut same = if opt.flat { all & !1 } else { 0 };
    for c in sc.cmds.iter() {
        if c.win.x >= ox + 32 || c.win.z <= ox {
            continue;
        }
        let win = rows(c.win.y - oy, c.win.w - oy);
        let kind = c.par.x;
        let flat = if c.xs != 0 {
            0
        } else if kind == k::K_SOLID {
            win
        } else if kind == k::K_RRECT || kind == k::K_BORDER || kind == k::K_SHADOW || (kind == k::K_GRAD && c.rad.w == 0) {
            if cfg!(feature = "mutant_flat") { win } else { rows(c.core.y - oy, c.core.w - oy) & win }
        } else {
            0
        };
        same &= (flat & flat << 1) | (!win & !(win << 1));
    }
    let need = all & !same;
    let mut m = need;
    while m != 0 {
        let y = m.trailing_zeros() as usize;
        m &= m - 1;
        sc.acc[y * 32..y * 32 + 32].fill([0; 4]);
    }
    // row buffers, shared by every command of the tile (each use writes what it reads)
    let (mut sx, mut cov, mut cov2) = ([0u16; 32], [0u16; 32], [0u16; 32]);
    for c in sc.cmds.iter() {
        let (x0, x1) = ((c.win.x - ox).max(kx0), (c.win.z - ox).min(kx1));
        if x0 >= x1 {
            continue;
        }
        let kind = c.par.x;
        // the core's colour where it is one colour (solid, rounded, border, shadow)
        let cv = if kind == k::K_BORDER { c.pm2 } else { c.pm };
        let (pm, pm2) = (st(c.pm), st(c.pm2));
        // a shadow's column profile over this tile's columns, made at the first row that needs it
        let (mut zone_x, mut sx_ready) = (0u32, false);
        let (hx, hy, lim) = ((c.b.z - c.b.x) >> 1, (c.b.w - c.b.y) >> 1, c.rad.y + (c.rad.y >> 1));
        let cz = if cfg!(feature = "mutant_core") { c.core.z + 1 } else { c.core.z };
        let (cx0, cx1) = ((c.core.x - ox).clamp(x0, x1), (cz - ox).clamp(x0, x1));
        let mut m = rows(c.win.y - oy, c.win.w - oy) & need;
        if kind == k::K_GLYPH && c.xs == 0 {
            // a glyph: its column range and first texel are the same on every row; each row is a coverage
            // row of its texels under one vector blend
            let (gw, gh) = ((c.b.z - c.b.x) >> 8, (c.b.w - c.b.y) >> 8);
            let tx0 = ((ox + x0) * 256 + 128 - c.b.x) >> 8;
            let (xa, xb) = (x0 + (-tx0).max(0), x1.min(x0 + gw - tx0));
            if xa >= xb {
                continue;
            }
            let (n, tcol, aw) = ((xb - xa) as usize, (tx0 + (xa - x0)) as usize, atlas.w as usize);
            while m != 0 {
                let y = m.trailing_zeros() as i32;
                m &= m - 1;
                let ty = ((oy + y) * 256 + 128 - c.b.y) >> 8;
                if ty < 0 || ty >= gh {
                    continue;
                }
                let first = c.par.z as usize + ty as usize * aw + tcol;
                match atlas.px.get(first..first + n) {
                    // the row of texels lies inside the atlas (always, for a valid region)
                    Some(t) => {
                        if !simd::q15_row(t, &mut cov[..n]) {
                            continue;
                        }
                    }
                    // bounds by construction: clamp each index, as the kernel's fetch does
                    None => {
                        for (i, o) in cov[..n].iter_mut().enumerate() {
                            *o = k::q15(atlas.px[(first + i).min(atlas.px.len() - 1)] as u32) as u16;
                        }
                    }
                }
                simd::blend_cov(&mut sc.acc[(y * 32 + xa) as usize..(y * 32 + xb) as usize], pm, &cov[..n]);
            }
            continue;
        }
        while m != 0 {
            let y = m.trailing_zeros() as i32;
            m &= m - 1;
            let gy = oy + y;
            let row = &mut sc.acc[(y * 32) as usize..(y * 32 + 32) as usize];
            if c.xs != 0 || kind == k::K_IMAGE || kind == k::K_RAMP {
                // a transformed space or a sampler: the kernel's `shade`, pixel by pixel
                for x in x0..x1 {
                    let p = &mut row[x as usize];
                    *p = st(k::blend(ld(p), k::shade(*c, ox + x, gy, atlas.fetch(c, ox + x, gy), now)));
                }
                continue;
            }
            if kind == k::K_SOLID {
                over(&mut row[x0 as usize..x1 as usize], c.pm);
                continue;
            }
            // a box kind: edge pixels through the kernel (a coverage row, then one vector blend), the core
            // as one span
            let (a, b) = if gy >= c.core.y && gy < c.core.w && cx0 < cx1 { (cx0, cx1) } else { (x1, x1) };
            let cy = gy * 256 + 128;
            let px0 = |xa: i32| (ox + xa) * 256 + 128;
            // a shadow away from its corners is the product of a column profile and a row value
            let shadow_row = if kind == k::K_SHADOW {
                if !sx_ready {
                    for (i, o) in sx[..(x1 - x0) as usize].iter_mut().enumerate() {
                        let px = px0(x0 + i as i32) - (c.b.x + hx);
                        *o = k::span3(px, hx, c.rad.y, c.rad.z) as u16;
                        if c.rad.x != 0 && px.abs() > hx - c.rad.x - lim {
                            zone_x |= 1 << i;
                        }
                    }
                    sx_ready = true;
                }
                let py = cy - (c.b.y + hy);
                Some((k::span3(py, hy, c.rad.y, c.rad.z) as u16, c.rad.x != 0 && py.abs() > hy - c.rad.x - lim))
            } else {
                None
            };
            let mut edge = |r: &mut [[u16; 4]], xa: i32| {
                let (n, cxa) = (r.len(), px0(xa));
                if n == 0 {
                    return;
                }
                match kind {
                    k::K_RRECT => {
                        for (i, o) in cov[..n].iter_mut().enumerate() {
                            *o = k::cov(k::sd(cxa + i as i32 * 256, cy, c.b, c.rad.x, c.par.y)) as u16;
                        }
                        simd::blend_cov(r, pm, &cov[..n]);
                    }
                    k::K_BORDER => {
                        for i in 0..n {
                            let d = k::sd(cxa + i as i32 * 256, cy, c.b, c.rad.x, c.par.y);
                            let outer = k::cov(d);
                            let inner = k::cov(d + c.rad.y).min(outer);
                            (cov[i], cov2[i]) = ((outer - inner) as u16, inner as u16);
                        }
                        simd::blend_cov2(r, pm, &cov[..n], pm2, &cov2[..n]);
                    }
                    k::K_SHADOW => {
                        let (sy, zone_y) = shadow_row.unwrap();
                        let i0 = (xa - x0) as usize;
                        simd::scale_row(&sx[i0..i0 + n], sy, &mut cov[..n]);
                        if zone_y {
                            // the corner corrections: the kernel, pixel by pixel, where a corner is in reach
                            let mut m = (zone_x >> i0) & (u32::MAX >> (32 - n));
                            while m != 0 {
                                let i = m.trailing_zeros() as usize;
                                m &= m - 1;
                                cov[i] = k::shadow(*c, cxa + i as i32 * 256, cy) as u16;
                            }
                        }
                        simd::blend_cov(r, pm, &cov[..n]);
                    }
                    // a colour and a distance per pixel: the kernel's own form is the fastest here (the split
                    // into a colour row and a coverage row measured 2x slower: 50 against 24.5 instr/px)
                    k::K_GRAD => {
                        for (i, p) in r.iter_mut().enumerate() {
                            *p = st(k::blend(ld(p), k::sh_grad(*c, cxa + i as i32 * 256, cy)));
                        }
                    }
                    _ => {
                        for (i, p) in r.iter_mut().enumerate() {
                            *p = st(k::blend(ld(p), k::sh_user(*c, cxa + i as i32 * 256, cy, now)));
                        }
                    }
                }
            };
            edge(&mut row[x0 as usize..a as usize], x0);
            if a < b {
                let mid = &mut row[a as usize..b as usize];
                match kind {
                    // a vertical gradient is one colour per row
                    k::K_GRAD if c.rad.z == 0 => over(mid, k::grad_color(*c, px0(a), cy)),
                    // the core's colour varies but its coverage is ONE
                    k::K_GRAD => {
                        for (i, p) in mid.iter_mut().enumerate() {
                            *p = st(k::blend(ld(p), k::grad_color(*c, px0(a) + i as i32 * 256, cy)));
                        }
                    }
                    k::K_USER => {
                        for (i, p) in mid.iter_mut().enumerate() {
                            *p = st(k::blend(ld(p), k::user_color(*c, px0(a) + i as i32 * 256, cy, now)));
                        }
                    }
                    _ => over(mid, cv),
                }
                edge(&mut row[b as usize..x1 as usize], b);
            }
        }
    }
    let (ka, kb) = (kx0 as usize, kx1 as usize);
    let mut m = all;
    while m != 0 {
        let y = m.trailing_zeros() as usize;
        m &= m - 1;
        if need >> y & 1 != 0 {
            store_row(&sc.acc[y * 32 + ka..y * 32 + kb], &mut dst[y * stride + ka..y * stride + kb]);
        } else {
            let (above, row) = dst.split_at_mut(y * stride);
            let src = &above[(y - 1) * stride + ka..(y - 1) * stride + kb];
            // a whole tile row is the common case: a fixed-size copy (four vector moves, no call)
            match (<&mut [u32; 32]>::try_from(&mut row[ka..kb]), <&[u32; 32]>::try_from(src)) {
                (Ok(d), Ok(s)) => *d = *s,
                _ => row[ka..kb].copy_from_slice(src),
            }
        }
    }
    sc.st.tiles += 1;
    sc.st.entries += sc.cmds.len() as u64;
    sc.st.rows_shaded += need.count_ones() as u64;
    sc.st.rows_copied += (all & same).count_ones() as u64;
}

/// Workers for bands of tile rows. `run` hands every worker (and the caller, as worker 0) the same closure;
/// the closure claims bands through an atomic counter, so one core and many cores run the same loop.
pub struct Pool {
    sh: Arc<Shared>,
    threads: Vec<std::thread::JoinHandle<()>>,
    scratch: Vec<Mutex<Scratch>>,
}

struct Shared {
    st: Mutex<State>,
    wake: Condvar,
    done: Condvar,
}

struct Job(*const (dyn Fn(usize) + Sync));
// SAFETY: the pointee is `Sync` and `Pool::run` keeps it alive until every worker has finished with it.
unsafe impl Send for Job {}

struct State {
    epoch: u64,
    job: Option<Job>,
    /// Workers asked to take part in the current job (ids 1..=want).
    want: usize,
    active: usize,
    quit: bool,
}

impl Pool {
    /// A pool with `workers` threads besides the caller.
    pub fn new(workers: usize) -> Pool {
        let sh = Arc::new(Shared { st: Mutex::new(State { epoch: 0, job: None, want: 0, active: 0, quit: false }), wake: Condvar::new(), done: Condvar::new() });
        let threads = (1..=workers)
            .map(|id| {
                let sh = sh.clone();
                std::thread::spawn(move || {
                    let mut seen = 0;
                    loop {
                        let f = {
                            let mut g = sh.st.lock().unwrap();
                            loop {
                                if g.quit {
                                    return;
                                }
                                if g.epoch != seen {
                                    seen = g.epoch;
                                    if id <= g.want {
                                        break;
                                    }
                                }
                                g = sh.wake.wait(g).unwrap();
                            }
                            g.job.as_ref().unwrap().0
                        };
                        // SAFETY: `run` does not return (and so the closure is not dropped) before `active`
                        // reaches 0, which this worker only decrements after the call.
                        unsafe { (*f)(id) };
                        let mut g = sh.st.lock().unwrap();
                        g.active -= 1;
                        if g.active == 0 {
                            sh.done.notify_one();
                        }
                    }
                })
            })
            .collect();
        Pool { sh, threads, scratch: (0..=workers).map(|_| Mutex::new(Scratch::default())).collect() }
    }
    pub fn workers(&self) -> usize {
        self.threads.len()
    }
    /// Run `f(worker)` on the caller and on `extra` workers (at most the pool's); returns when all are done.
    pub fn run(&self, extra: usize, f: &(dyn Fn(usize) + Sync)) {
        let extra = extra.min(self.threads.len());
        if extra > 0 {
            let mut g = self.sh.st.lock().unwrap();
            // SAFETY (lifetime erasure): the pointer is only dereferenced by workers counted in `active`,
            // and this function waits for `active == 0` before returning.
            g.job = Some(Job(unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), *const (dyn Fn(usize) + Sync + 'static)>(f) }));
            g.want = extra;
            g.active = extra;
            g.epoch += 1;
            drop(g);
            self.sh.wake.notify_all();
        }
        f(0);
        if extra > 0 {
            let mut g = self.sh.st.lock().unwrap();
            while g.active != 0 {
                g = self.sh.done.wait(g).unwrap();
            }
            g.job = None;
        }
    }
    fn take_stats(&self) -> Stats {
        let mut s = Stats::default();
        for m in &self.scratch {
            let mut g = m.lock().unwrap();
            s.add(&g.st);
            g.st = Stats::default();
        }
        s
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.sh.st.lock().unwrap().quit = true;
        self.sh.wake.notify_all();
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// The fast executor: repair the program's tiles in `out` (`stride` words per row), on the caller plus
/// `extra` pool workers. The unit of work is a band (one row of tiles); bands are disjoint `&mut` slices of
/// `out`, which is the whole proof that they may be written concurrently.
pub fn raster(scene: &Scene, prog: &Prog, atlas: &Atlas, out: &mut [u32], stride: usize, pool: &Pool, extra: usize, opt: Opt) -> Stats {
    let (fw, fh, now, nt) = (prog.fw() as usize, prog.fh() as usize, prog.now(), prog.tiles());
    if extra == 0 {
        // one core: the same per-tile loop with no band bookkeeping (a two-tile repair should not pay for a
        // vector of bands and their locks)
        let mut sc = pool.scratch[0].lock().unwrap();
        for t in 0..nt {
            let (ox, oy, list) = prog.tile(t);
            sc.cmds.clear();
            sc.cmds.extend(list.iter().map(|&i| scene.load_at(i, now)));
            let (cols, nrows) = ((fw - ox as usize).min(32), (fh - oy as usize).min(32));
            shade_tile(&mut sc, ox, oy, atlas, now, &mut out[oy as usize * stride + ox as usize..], stride, cols, nrows, opt);
        }
        let st = sc.st;
        sc.st = Stats::default();
        return st;
    }
    // bands: (first tile, end tile, the band's rows of `out`)
    let mut bands: Vec<Mutex<Option<(usize, usize, &mut [u32])>>> = Vec::new();
    let mut i = 0;
    for (ty, chunk) in out.chunks_mut(32 * stride).enumerate() {
        if i >= nt {
            break;
        }
        let j0 = i;
        while i < nt && prog.words[HDR + 4 * i + 1] as usize == ty * 32 {
            i += 1;
        }
        if i > j0 {
            bands.push(Mutex::new(Some((j0, i, chunk))));
        }
    }
    let next = AtomicUsize::new(0);
    let work = |id: usize| {
        let mut sc = pool.scratch[id].lock().unwrap();
        loop {
            let b = next.fetch_add(1, Relaxed);
            if b >= bands.len() {
                break;
            }
            let Some((t0, t1, px)) = bands[b].lock().unwrap().take() else { continue };
            for t in t0..t1 {
                let (ox, oy, list) = prog.tile(t);
                sc.cmds.clear();
                sc.cmds.extend(list.iter().map(|&i| scene.load_at(i, now)));
                let (cols, nrows) = ((fw - ox as usize).min(32), (fh - oy as usize).min(32));
                shade_tile(&mut sc, ox, oy, atlas, now, &mut px[ox as usize..], stride, cols, nrows, opt);
            }
        }
    };
    // a small repair is not worth a wake: fewer bands than workers uses fewer workers
    pool.run(extra.min(bands.len().saturating_sub(1)), &work);
    pool.take_stats()
}

/// The gates' oracle for large scenes: the specification per tile (every pixel folds its tile's whole,
/// uncut list through `k::shade`), across the pool. Equal to `naive` by identity 1 alone.
pub fn reference(scene: &Scene, fw: u32, fh: u32, atlas: &Atlas, now: u32, out: &mut [u32], stride: usize, pool: &Pool) {
    let mut prog = Prog::default();
    Binner::default().bin(scene, fw, fh, stride as u32, atlas, now, &Damage::All, false, &mut prog);
    raster(scene, &prog, atlas, out, stride, pool, pool.workers(), Opt { flat: false, spec: true, clip: EVERYWHERE });
}

/// `reference` at clock 0 (a scene with no transitions).
pub fn naive_tiles(scene: &Scene, fw: u32, fh: u32, atlas: &Atlas, out: &mut [u32], stride: usize, pool: &Pool) {
    reference(scene, fw, fh, atlas, 0, out, stride, pool);
}

/// The specification: every pixel of the frame folds every command of the scene, in order, with no binning,
/// no cut, no core, no span (identities 1-4 are what make `raster` and the GPU equal to this).
pub fn naive(scene: &Scene, fw: u32, fh: u32, atlas: &Atlas, now: u32, out: &mut [u32], stride: usize) {
    let cmds: Vec<Cmd> = (0..scene.len() as u32).filter(|&i| scene.head(i) & F_TO == 0).map(|i| scene.load_at(i, now)).collect();
    for y in 0..fh as i32 {
        for x in 0..fw as i32 {
            let mut acc = k::splat(0);
            for c in &cmds {
                acc = k::blend(acc, k::shade(*c, x, y, atlas.fetch(c, x, y), now));
            }
            out[y as usize * stride + x as usize] = k::store8(acc);
        }
    }
}

const _: () = assert!(WORDS == 32);

/// Identity 5 as a key: a 128-bit hash of a tile's program *relative to the tile's origin*, with every
/// command reduced to what the tile can see of it (its window clamped to the tile; a command whose visible
/// part lies inside its core is just its colour). Tiles with equal keys hold equal pixels: rows of a list,
/// cells of a grid, every tile of a plain background.
pub fn tile_key(scene: &Scene, list: &[u32], ox: i32, oy: i32, now: u32) -> u128 {
    let mut h = (0x9e37_79b9_7f4a_7c15u64, 0xc2b2_ae3d_27d4_eb4fu64);
    let mut mix = |v: u64| {
        h.0 = (h.0 ^ v).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(29);
        h.1 = (h.1.rotate_left(23) ^ v).wrapping_mul(0xff51_afd7_ed55_8ccd);
    };
    let p4 = |a: i32, b: i32, c: i32, d: i32| (a as u16 as u64) | (b as u16 as u64) << 16 | (c as u16 as u64) << 32 | (d as u16 as u64) << 48;
    let c4 = |v: Vec4u| (v.x as u64) | (v.y as u64) << 16 | (v.z as u64) << 32 | (v.w as u64) << 48;
    for &i in list {
        let c = scene.load_at(i, now);
        let (x0, y0, x1, y1) = ((c.win.x - ox).clamp(0, 32), (c.win.y - oy).clamp(0, 32), (c.win.z - ox).clamp(0, 32), (c.win.w - oy).clamp(0, 32));
        if x0 >= x1 || y0 >= y1 {
            continue;
        }
        mix(p4(x0, y0, x1, y1));
        let kind = c.par.x;
        let in_core = c.core.x <= ox + x0 && c.core.y <= oy + y0 && c.core.z >= ox + x1 && c.core.w >= oy + y1;
        if kind == k::K_SOLID || (in_core && (kind == k::K_RRECT || kind == k::K_BORDER || kind == k::K_SHADOW)) {
            mix(c4(if kind == k::K_BORDER { c.pm2 } else { c.pm }));
            continue;
        }
        mix(kind as u64 | (c.par.y as u64) << 8 | (c.par.z as u64) << 16 | (c.par.w as u64) << 48);
        mix(p4((c.core.x - ox).clamp(0, 32), (c.core.y - oy).clamp(0, 32), (c.core.z - ox).clamp(0, 32), (c.core.w - oy).clamp(0, 32)));
        mix(((c.b.x - ox * 256) as u32 as u64) | ((c.b.y - oy * 256) as u32 as u64) << 32);
        mix(((c.b.z - ox * 256) as u32 as u64) | ((c.b.w - oy * 256) as u32 as u64) << 32);
        mix((c.rad.x as u32 as u64) | (c.rad.y as u32 as u64) << 32);
        mix((c.rad.z as u32 as u64) | (c.rad.w as u32 as u64) << 32);
        mix(c4(c.pm));
        mix(c4(c.pm2));
        if kind == k::K_USER {
            mix(now as u64);
        }
        if c.xs != 0 {
            mix((c.xi.x as u32 as u64) | (c.xi.y as u32 as u64) << 32);
            mix((c.xi.z as u32 as u64) | (c.xi.w as u32 as u64) << 32);
            mix(((c.xk.x - ox * 256) as u32 as u64) | ((c.xk.y - oy * 256) as u32 as u64) << 32);
            mix(((c.xk.z - ox * 256) as u32 as u64) | ((c.xk.w - oy * 256) as u32 as u64) << 32);
            mix(c.xs as u64);
        }
    }
    (h.0 as u128) << 64 | h.1 as u128
}

/// (tiles, distinct keys) of a program: how much of a frame is repetition (E11).
pub fn distinct_tiles(scene: &Scene, prog: &Prog) -> (usize, usize) {
    let mut keys: Vec<u128> = (0..prog.tiles())
        .map(|t| {
            let (ox, oy, list) = prog.tile(t);
            tile_key(scene, list, ox, oy, prog.now())
        })
        .collect();
    keys.sort_unstable();
    keys.dedup();
    (prog.tiles(), keys.len())
}
