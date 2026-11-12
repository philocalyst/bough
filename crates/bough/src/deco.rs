//! What zgui draws beyond flat boxes and text (round 3b, input agent): linear and radial gradients, box
//! shadows, vector icons and rotated images.
//!
//! All of them are one fused-kernel instance each, so they go through the same records, damage and repair
//! as every quad:
//!
//! * **images** (kind 4): premultiplied RGBA in the image atlas, drawn 1:1 at the instance's box, rotated
//!   about its centre by `radius` radians (bilinear). Vector icons are SVG documents parsed by usvg and
//!   rasterised by resvg at their device size, once per (document, size).
//! * **gradients** (kind 5): a 256-texel premultiplied ramp in the image atlas (CSS stop positions and
//!   premultiplied interpolation resolved on the CPU), sampled along a linear gradient line at an angle or
//!   a radial circle reaching the farthest corner, in a rounded box. The geometry is box-relative, so layout
//!   moves and resizes it like any quad.
//! * **shadows** (kind 6): a rounded box's Gaussian shadow (CSS `box-shadow`: offset, blur = 2 sigma,
//!   spread), drawn beneath its box.
//!
//! Ink: a shadow reaches `ceil(3 sigma) + 1` px beyond its box and a rotated image its circumscribed
//! circle; both are carried in the instance (`flags`, the kernel's margin) and the record's ink rect uses
//! the same value, so the kernel never writes a pixel outside the record's ink (exact damage).
use crate::elem::{Apply, IntoProp, Mounter};
use crate::rt::*;
use crate::elem::Element;
use bough_core::{NRect, RecordInit, KF_NOHIT};
use bough_frame::{IMG_H, IMG_W};
use bough_frame::Instance;
use std::collections::HashMap;

pub const KIND_IMAGE: u8 = 4;
pub const KIND_GRAD: u8 = 5;
pub const KIND_SHADOW: u8 = 6;
/// A decoration node (shadow, image): its record's ink is its box plus `margin`.
pub const NK_DECO: u8 = 7;

/// A CSS gradient: `stops` are (position 0..1, colour RGBA8 with r in the low byte, straight alpha).
#[derive(Clone, Copy, Debug)]
pub struct Grad {
    pub radial: bool,
    /// Linear: the CSS angle (0 = to top, clockwise).
    pub angle_deg: f32,
    /// Radial: the centre as fractions of the box (`circle at 30% 30%` = [0.3, 0.3]).
    pub at: [f32; 2],
    pub stops: &'static [(f32, u32)],
}

/// One CSS box shadow (px; blur is CSS's blur radius = 2 sigma).
#[derive(Clone, Copy, Debug)]
pub struct Shadow {
    pub dx: f32,
    pub dy: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: u32,
}

/// A layout node's decorations (template tables index these; `Deco::NONE` is entry 0).
#[derive(Clone, Copy, Debug)]
pub struct Deco {
    pub grad: Option<Grad>,
    /// In CSS order (the first is on top).
    pub shadows: &'static [Shadow],
    /// A vector icon (an SVG document; `currentColor` already resolved), drawn over the box.
    pub svg: &'static str,
    /// A CSS transform of the box and its subtree, about the box's centre (`transform-origin` 50% 50%).
    pub xform: Option<Xf>,
}

/// A CSS transform: `m` = [a, b, c, d] (row-major: x' = a x + b y), then a translation `t` (px at scale 1,
/// or fractions of the box's own size where `pct`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub m: [f32; 4],
    pub t: [f32; 2],
    pub pct: [bool; 2],
}

impl Deco {
    pub const NONE: Deco = Deco { grad: None, shadows: &[], svg: "", xform: None };
}

// ================================================================================== rasterising from process entry

/// SVG documents a cold start rasterises on worker threads from process entry (`app::prepare`): a strike's
/// glyphs' scheme for images. `Images::svg` takes a done one, rasterises one no worker started itself, or
/// waits for one in progress.
struct PreSvg {
    items: Vec<(&'static str, u32, u32)>,
    state: Vec<std::sync::atomic::AtomicU8>,
    done: std::sync::Mutex<Vec<Option<Vec<u8>>>>,
    cv: std::sync::Condvar,
    stats: std::sync::Mutex<[u64; 3]>,
}

static PRE_SVG: std::sync::Mutex<Option<std::sync::Arc<PreSvg>>> = std::sync::Mutex::new(None);

fn render_svg(svg: &str, w: u32, h: u32) -> Vec<u8> {
    let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("svg");
    let mut pm = resvg::tiny_skia::Pixmap::new(w.max(1), h.max(1)).expect("pixmap");
    let sz = tree.size();
    let t = resvg::tiny_skia::Transform::from_scale(w as f32 / sz.width(), h as f32 / sz.height());
    resvg::render(&tree, t, &mut pm.as_mut());
    pm.take()
}

/// Start rasterising `items` (document, device w, h) on `threads` workers.
pub fn prerender_svgs(mut items: Vec<(&'static str, u32, u32)>, threads: usize) {
    use std::sync::atomic::{AtomicU8, Ordering};
    let mut seen = std::collections::HashSet::new();
    items.retain(|&(s, w, h)| seen.insert((s, w, h)));
    let n = items.len();
    if n == 0 {
        return;
    }
    let pre = std::sync::Arc::new(PreSvg { items, state: (0..n).map(|_| AtomicU8::new(0)).collect(), done: std::sync::Mutex::new(vec![None; n]), cv: std::sync::Condvar::new(), stats: Default::default() });
    *PRE_SVG.lock().unwrap() = Some(pre.clone());
    for w in 0..threads.clamp(1, 8).min(n) {
        let p = pre.clone();
        std::thread::Builder::new()
            .name(format!("bough-svg-{w}"))
            .spawn(move || {
                let (i0, t0) = (bough_sys::perf::thread_instructions(), std::time::Instant::now());
                let mut k = 0;
                for i in 0..p.items.len() {
                    if p.state[i].compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire).is_ok() {
                        let (s, w, h) = p.items[i];
                        let px = render_svg(s, w, h);
                        p.done.lock().unwrap()[i] = Some(px);
                        p.state[i].store(2, Ordering::Release);
                        p.cv.notify_all();
                        k += 1;
                    }
                }
                let mut st = p.stats.lock().unwrap();
                st[0] += k;
                st[1] += bough_sys::perf::thread_instructions() - i0;
                st[2] += t0.elapsed().as_nanos() as u64;
            })
            .expect("svg thread");
    }
}

/// Workers of [`prerender_svgs`] so far: (images, instructions, ns busy), summed.
pub fn prerender_svg_stats() -> [u64; 3] {
    PRE_SVG.lock().unwrap().as_ref().map(|p| *p.stats.lock().unwrap()).unwrap_or_default()
}

/// The prepared raster of (svg, w, h), if it was named: taken (or made here when no worker started it).
fn take_prepared(svg: &str, w: u32, h: u32) -> Option<Vec<u8>> {
    use std::sync::atomic::Ordering;
    let pre = PRE_SVG.lock().unwrap().clone()?;
    let i = pre.items.iter().position(|&(s, a, b)| (a, b) == (w, h) && std::ptr::eq(s, svg) || (a, b) == (w, h) && s == svg)?;
    if pre.state[i].compare_exchange(0, 3, Ordering::AcqRel, Ordering::Acquire).is_ok() {
        return None; // nobody started it: the caller rasterises (and it stays taken)
    }
    let mut d = pre.done.lock().unwrap();
    loop {
        if pre.state[i].load(Ordering::Acquire) == 3 {
            return None;
        }
        if let Some(v) = d[i].take() {
            pre.state[i].store(3, Ordering::Release);
            return Some(v);
        }
        d = pre.cv.wait(d).unwrap();
    }
}

// ================================================================================== the image atlas

/// The image atlas's CPU copy: shelves of premultiplied RGBA images and gradient ramps, uploaded by rows.
pub struct Images {
    px: Vec<u8>,
    x: u32,
    y: u32,
    row_h: u32,
    dirty: Option<(u32, u32)>,
    cache: HashMap<(u64, u32, u32), [u16; 4]>,
    pub rasterised: u32,
    /// Instructions the rasterising thread spent parsing and rasterising SVG (misses only).
    pub svg_instr: u64,
}

impl Default for Images {
    fn default() -> Self {
        Images { px: Vec::new(), x: 0, y: 0, row_h: 0, dirty: None, cache: HashMap::new(), rasterised: 0, svg_instr: 0 }
    }
}

fn hash(bytes: &[u8]) -> u64 {
    // FNV-1a
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
    }
    h
}

impl Images {
    /// The image atlas's CPU copy and its cache, as memory-ledger lines.
    pub fn ledger(&self, l: &mut bough_mem::Ledger) {
        use bough_mem::Scales::Content;
        l.vec("images", "image atlas (CPU copy, RGBA8)", &self.px, self.px.capacity(), Content);
        let c = self.cache.capacity() * (size_of::<((u64, u32, u32), [u16; 4])>() + 1);
        l.many("images", "image cache map", Content, c, c);
    }
    fn alloc(&mut self, w: u32, h: u32) -> (u32, u32) {
        if self.px.is_empty() {
            self.px = vec![0; (IMG_W * IMG_H * 4) as usize];
        }
        assert!(w + 1 <= IMG_W, "image wider than the atlas");
        if self.x + w + 1 > IMG_W {
            self.y += self.row_h + 1;
            self.x = 0;
            self.row_h = 0;
        }
        assert!(self.y + h <= IMG_H, "image atlas full");
        let at = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        let (a, b) = self.dirty.unwrap_or((at.1, at.1 + h));
        self.dirty = Some((a.min(at.1), b.max(at.1 + h)));
        at
    }

    /// The rows written since the last upload (taken).
    pub fn take_dirty(&mut self) -> Option<std::ops::Range<u32>> {
        self.dirty.take().map(|(a, b)| a..b)
    }

    pub fn pixels(&self) -> &[u8] {
        &self.px
    }

    /// Changes whenever an image or a ramp is added (the allocation cursor only advances): an executor that
    /// keeps its own copy of the atlas compares it with the stamp it last uploaded.
    pub fn stamp(&self) -> u64 {
        (self.y as u64) << 32 | self.x as u64
    }

    /// Place `w` x `h` premultiplied RGBA pixels (cached by `key`); returns (x, y, w, h) in the atlas.
    pub fn put(&mut self, key: u64, w: u32, h: u32, fill: impl FnOnce(&mut [u8])) -> [u16; 4] {
        if let Some(&r) = self.cache.get(&(key, w, h)) {
            return r;
        }
        let mut buf = vec![0u8; (w * h * 4) as usize];
        fill(&mut buf);
        let (x, y) = self.alloc(w, h);
        for row in 0..h {
            let d = ((y + row) * IMG_W + x) as usize * 4;
            self.px[d..d + w as usize * 4].copy_from_slice(&buf[(row * w * 4) as usize..((row + 1) * w * 4) as usize]);
        }
        let r = [x as u16, y as u16, w as u16, h as u16];
        self.cache.insert((key, w, h), r);
        r
    }

    /// An SVG document rasterised (resvg) to `w` x `h` px, its viewBox fitted as SVG does by default.
    pub fn svg(&mut self, svg: &str, w: u32, h: u32) -> [u16; 4] {
        let key = hash(svg.as_bytes());
        let mut n = 0;
        let i0 = bough_sys::perf::thread_instructions();
        let r = self.put(key, w.max(1), h.max(1), |buf| {
            n = 1;
            let px = take_prepared(svg, w.max(1), h.max(1)).unwrap_or_else(|| render_svg(svg, w, h));
            buf.copy_from_slice(&px);
        });
        self.rasterised += n;
        if n != 0 {
            self.svg_instr += bough_sys::perf::thread_instructions() - i0;
        }
        r
    }

    /// A gradient's 256-texel ramp: CSS stop positions resolved, colours interpolated premultiplied.
    pub fn ramp(&mut self, stops: &[(f32, u32)]) -> [u16; 4] {
        let mut kb = Vec::with_capacity(stops.len() * 8);
        for &(o, c) in stops {
            kb.extend_from_slice(&o.to_bits().to_le_bytes());
            kb.extend_from_slice(&c.to_le_bytes());
        }
        let stops = stops.to_vec();
        self.put(hash(&kb) ^ 0x5241_4d50, 256, 1, |buf| {
            let pm = |c: u32| {
                let a = (c >> 24) as f32 / 255.0;
                [(c & 255) as f32 / 255.0 * a, ((c >> 8) & 255) as f32 / 255.0 * a, ((c >> 16) & 255) as f32 / 255.0 * a, a]
            };
            for i in 0..256usize {
                let t = i as f32 / 255.0;
                let j = stops.iter().position(|s| s.0 >= t).unwrap_or(stops.len() - 1);
                let v = if j == 0 || stops[j].0 <= t {
                    pm(stops[j].1)
                } else {
                    let (a, b) = (stops[j - 1], stops[j]);
                    let f = ((t - a.0) / (b.0 - a.0).max(1e-6)).clamp(0.0, 1.0);
                    let (ca, cb) = (pm(a.1), pm(b.1));
                    [0, 1, 2, 3].map(|k| ca[k] + (cb[k] - ca[k]) * f)
                };
                for k in 0..4 {
                    buf[i * 4 + k] = (v[k] * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        })
    }
}

// ================================================================================== margins and instances

/// Ink margin (px, the kernel's `flags`) of a shadow of blur sigma `sigma`: 4 sigma (where the kernel's erf
/// saturates; its y window is 3 sigma) plus a pixel.
pub fn shadow_margin(sigma: f32) -> u8 {
    ((4.0 * sigma).ceil() + 1.0).clamp(1.0, 255.0) as u8
}

/// Ink margin of an image that may rotate: its circumscribed circle beyond the box, plus a texel.
pub fn rot_margin(w: f32, h: f32) -> u8 {
    ((0.5 * (w * w + h * h).sqrt() - 0.5 * w.min(h)).ceil() + 1.0).clamp(1.0, 255.0) as u8
}

/// The instance of a shadow of the box (x, y, w, h) with corner radius `r` (px).
pub fn shadow_instance(s: &Shadow, scale: f32, space: u16, r: f32) -> (Instance, f32) {
    let sigma = 0.5 * s.blur * scale;
    if sigma <= 0.0 {
        // no blur: the box grown by the spread, a rounded rect
        let ins = Instance { color: s.color, radius: (r + s.spread * scale).max(0.0), space, kind: 1, ..Default::default() };
        return (ins, 1.0);
    }
    let m = shadow_margin(sigma);
    let ins = Instance { color: s.color, radius: (r + s.spread * scale).max(0.0), bw: sigma, space, kind: KIND_SHADOW, flags: m, ..Default::default() };
    (ins, m as f32)
}

/// Make `ins` (a box's quad) a gradient fill.
pub fn gradient_instance(ins: &mut Instance, g: &Grad, ramp: [u16; 4]) {
    ins.kind = KIND_GRAD;
    ins.uv = [ramp[0], ramp[1], g.radial as u16, 0];
    ins.bw = if g.radial { 0.0 } else { g.angle_deg.to_radians() };
    ins.color = if g.radial { g.at[0].to_bits() } else { 0 };
    ins.color2 = if g.radial { g.at[1].to_bits() } else { 0 };
    ins.flags = 0;
}

/// Mount one decoration record (its own node, run, instance and handle) at px box (x, y, w, h).
pub(crate) fn mount_deco<V>(m: &mut Mounter<'_, V>, ins: Instance, x: f32, y: f32, w: f32, h: f32, margin: f32) -> u32 {
    // the geometry grid (1/256 px): sums exact on the GPU, moved edges compared exactly (rt::quad_geom)
    let (x, y, w, h) = (snap(x), snap(y), snap(w), snap(h));
    let node = m.rt.alloc_node();
    let run = m.rt.alloc_run(1);
    m.rt.runs[run as usize].len = 1;
    let inst = m.rt.runs[run as usize].first;
    let mut ins = ins;
    ins.pos = [x, y];
    ins.size = [w, h];
    ins.space = m.space;
    *m.rt.inst_mut(inst) = ins;
    let hd = m.rt.bough.new_handle();
    let ink = m.rt.ink_rect(m.space, x, y, w, h, margin);
    {
        let n = &mut m.rt.nodes[node as usize];
        n.handle = hd;
        n.run = run;
        n.space = m.space;
        n.clip = m.clip;
        n.kind = NK_DECO;
        n.x = x;
        n.y = y;
        n.w = w;
        n.h = h;
        n.margin = margin;
    }
    m.scope.nodes.push(node);
    m.scope.runs.push(run);
    m.recs.push(RecordInit { depth: m.depth, ink: ink.to_nrect(), kind: REC_QUAD | KF_NOHIT, payload: inst, handle: hd.idx, clip: m.clip });
    node
}

impl Rt {
    /// Move / resize decoration node `node` to px box (x, y, w, h) in its space (snapped to the geometry grid):
    /// the damage is the bands around the edges that moved, each kind's band from `rt::edge_band_of`.
    pub(crate) fn place_deco(&mut self, node: u32, x: f32, y: f32, w: f32, h: f32) {
        self.quad_geom(node, x, y, w, h);
    }
}

// ================================================================================== elements

/// Rotation of an image node: the instance's angle word is its paint state (a paint write).
pub struct ApplyAngle;
impl Apply for ApplyAngle {
    type P = f32;
    fn apply(rt: &mut Rt, node: u32, v: &f32) {
        let n = &rt.nodes[node as usize];
        let (h, inst) = (n.handle, rt.runs[n.run as usize].first);
        let p = rt.inst_mut(inst);
        let old = p.radius;
        if old == *v {
            return;
        }
        p.radius = *v;
        rt.paint_write(h, old.to_bits() as u64, v.to_bits() as u64);
    }
}

/// An SVG image at (x, y) of w x h points, optionally rotating (`angle` in radians, constant or wired).
pub struct Image<A> {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    svg: &'static str,
    angle: A,
    rotates: bool,
}

pub fn svg_image(x: f32, y: f32, w: f32, h: f32, svg: &'static str) -> Image<f32> {
    Image { x, y, w, h, svg, angle: 0.0, rotates: false }
}

impl<A> Image<A> {
    /// Rotate about the centre (radians; a wired signal makes it a paint-only animation).
    pub fn angle<B>(self, a: B) -> Image<B> {
        Image { x: self.x, y: self.y, w: self.w, h: self.h, svg: self.svg, angle: a, rotates: true }
    }
}

impl<V: 'static, A: IntoProp<f32, V>> Element<V> for Image<A> {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let s = m.rt.scale;
        let (wp, hp) = ((self.w * s).round().max(1.0), (self.h * s).round().max(1.0));
        let uv = m.rt.images.svg(self.svg, wp as u32, hp as u32);
        let margin = if self.rotates { rot_margin(wp, hp) } else { 0 };
        let ins = Instance { color: u32::MAX, uv, kind: KIND_IMAGE, flags: margin, ..Default::default() };
        let (x, y) = ((m.ox + self.x) * s, (m.oy + self.y) * s);
        let node = mount_deco(m, ins, x, y, wp, hp, margin.max(1) as f32);
        let a = self.angle.lower::<ApplyAngle>(m, node);
        let inst = m.rt.runs[m.rt.nodes[node as usize].run as usize].first;
        m.rt.inst_mut(inst).radius = a;
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}

/// A box (x, y, w, h points, corner radius) with CSS box shadows beneath and a fill: a gradient, or
/// a solid colour (`grad: None`).
pub struct DecoBox {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    bg: u32,
    grad: Option<Grad>,
    shadows: &'static [Shadow],
}

pub fn deco_box(x: f32, y: f32, w: f32, h: f32, radius: f32) -> DecoBox {
    DecoBox { x, y, w, h, radius, bg: 0, grad: None, shadows: &[] }
}

impl DecoBox {
    pub fn bg(mut self, c: crate::color::Color) -> Self {
        self.bg = c.0;
        self
    }
    pub fn gradient(mut self, g: Grad) -> Self {
        self.grad = Some(g);
        self
    }
    pub fn shadows(mut self, s: &'static [Shadow]) -> Self {
        self.shadows = s;
        self
    }
}

impl<V: 'static> Element<V> for DecoBox {
    fn mount(self, m: &mut Mounter<'_, V>) {
        // state lives in signals, the atlas or the region: a re-description steps over it
        crate::describe::mount_opaque(m, move |m: &mut Mounter<'_, V>| {
        let s = m.rt.scale;
        let (x, y, w, h, r) = ((m.ox + self.x) * s, (m.oy + self.y) * s, self.w * s, self.h * s, self.radius * s);
        for sh in self.shadows.iter().rev() {
            let (ins, margin) = shadow_instance(sh, s, m.space, r);
            let g = sh.spread * s;
            mount_deco(m, ins, x + sh.dx * s - g, y + sh.dy * s - g, w + 2.0 * g, h + 2.0 * g, margin);
        }
        let mut ins = Instance { color: self.bg, radius: r, kind: if r > 0.0 { 1 } else { 0 }, ..Default::default() };
        let mut margin = if r > 0.0 { 1.0 } else { 0.0 };
        if let Some(g) = &self.grad {
            let ramp = m.rt.images.ramp(g.stops);
            gradient_instance(&mut ins, g, ramp);
            margin = 1.0;
        }
        mount_deco(m, ins, x, y, w, h, margin);
    })
    }
    fn update(self, m: &mut Mounter<'_, V>) {
        crate::describe::update_opaque(m);
    }
}

/// Damage a fresh ink rect (helper for tests of exactness: the record's ink as the damage model sees it).
pub fn ink_of(rt: &Rt, node: u32) -> NRect {
    let n = &rt.nodes[node as usize];
    rt.ink_rect(n.space, n.x, n.y, n.w, n.h, n.margin).to_nrect()
}
